//! Outbound configured-peer delivery.
//!
//! This adapter owns only the remote connection setup and peer provenance
//! header. The request and response wire protocol is the canonical ATM HTTP
//! path shared with local CLI and graft clients.

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, mpsc};
use std::thread;
use std::time::Duration;

use atm_core::api::{RequestDeadline, read_http_response, write_http_request_with_headers};
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::WriteRequest;
use atm_storage::PeerEndpoint;

use crate::peer_http_listener::{
    PEER_HTTP_LOCAL_RESPONSE_BUDGET, PLAINTEXT_PEER_SOURCE_HOST_HEADER, PeerHttpRuntimeConfig,
};

/// A stalled OS resolver cannot be cancelled through `ToSocketAddrs`. Keep the
/// resulting abandoned helper threads bounded; callers fail closed instead of
/// queueing once this cap is reached.
const MAX_IN_FLIGHT_PEER_RESOLVES: usize = 4;

static IN_FLIGHT_PEER_RESOLVES: LazyLock<Arc<AtomicUsize>> =
    LazyLock::new(|| Arc::new(AtomicUsize::new(0)));

/// Sends one ordinary write through the canonical `/v1/atm/messages` request
/// format. The peer listener accepts this same singleton representation before
/// optionally accepting a recovery `messages[]` representation.
pub(crate) fn send_configured_peer_write(
    config: &PeerHttpRuntimeConfig,
    endpoint: &PeerEndpoint,
    write: &WriteRequest,
    deadline: RequestDeadline,
) -> Result<SendResponseEnvelope, AtmError> {
    let correlation_id = write
        .origin_message_id
        .map(|message_id| message_id.to_string())
        .unwrap_or_else(|| "missing-origin-message-id".to_owned());
    tracing::debug!(
        subsystem = "peer_http",
        action = "send_write",
        %correlation_id,
        canonical_host = %endpoint.canonical_host,
        port = endpoint.port.get(),
        "opening configured peer HTTP connection"
    );

    let mut stream = connect_configured_peer(endpoint, deadline)?;
    stream.set_nodelay(true).map_err(|source| {
        peer_delivery_failure("disable Nagle buffering for configured peer", source)
    })?;
    let remaining = peer_response_budget(deadline)?;
    stream.set_read_timeout(Some(remaining)).map_err(|source| {
        peer_delivery_failure("configure configured-peer response timeout", source)
    })?;
    stream
        .set_write_timeout(Some(remaining))
        .map_err(|source| {
            peer_delivery_failure("configure configured-peer write timeout", source)
        })?;

    let request = RequestEnvelope::Write(Box::new(write.clone()));
    write_http_request_with_headers(
        &mut stream,
        &request,
        &[(
            PLAINTEXT_PEER_SOURCE_HOST_HEADER,
            config.source_host.as_str(),
        )],
    )
    .map_err(|error| {
        AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but write to configured peer `{}` failed: {error}",
            endpoint.canonical_host
        ))
    })?;

    let response = read_http_response(&mut stream, &request).map_err(|error| {
        AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but configured peer `{}` did not return one valid write response: {error}",
            endpoint.canonical_host
        ))
    })?;
    ensure_matching_send_response(&response, write, endpoint)?;
    let ResponseEnvelope::Send(response) = response else {
        return Err(AtmError::remote_delivery_unconfirmed(format!(
            "configured peer `{}` returned no send response for the submitted write",
            endpoint.canonical_host
        )));
    };
    tracing::debug!(
        subsystem = "peer_http",
        action = "send_write",
        %correlation_id,
        message_id = ?write.origin_message_id,
        canonical_host = %endpoint.canonical_host,
        "configured peer HTTP write response confirmed"
    );
    Ok(response)
}

/// Resolves through the operating system and spends the caller's remaining
/// request budget across every returned address. There is no resolver thread,
/// connection pool, or retry state here; this is one bounded direct attempt.
fn connect_configured_peer(
    endpoint: &PeerEndpoint,
    deadline: RequestDeadline,
) -> Result<TcpStream, AtmError> {
    let addresses = resolve_configured_peer(endpoint, peer_response_budget(deadline)?)?;
    let mut last_failure = None;
    for address in addresses {
        let remaining = peer_response_budget(deadline)?;
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(stream) => return Ok(stream),
            Err(source) => last_failure = Some((address, source)),
        }
    }
    match last_failure {
        Some((address, source)) => Err(AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but connect to configured peer address `{address}` failed: {source}"
        ))),
        None => Err(AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but configured peer `{}` resolved to no connectable addresses",
            endpoint.canonical_host
        ))),
    }
}

fn resolve_configured_peer(
    endpoint: &PeerEndpoint,
    timeout: Duration,
) -> Result<Vec<SocketAddr>, AtmError> {
    let host = endpoint.canonical_host.to_string();
    let port = endpoint.port.get();
    resolve_with_timeout(Arc::clone(&IN_FLIGHT_PEER_RESOLVES), timeout, move || {
        (host.as_str(), port)
            .to_socket_addrs()
            .map(Iterator::collect)
    })
}

/// Performs one OS-backed lookup without allowing it to strand the fixed
/// dispatch worker pool. This owns no resolver service, queue, timer, retry
/// policy, or persistent worker: each attempt creates one helper and returns
/// when its bounded receive expires. A truly hung OS lookup can retain that
/// helper until the OS returns, but the shared permit cap bounds concurrent
/// abandoned helpers.
fn resolve_with_timeout<F>(
    in_flight: Arc<AtomicUsize>,
    timeout: Duration,
    resolve: F,
) -> Result<Vec<SocketAddr>, AtmError>
where
    F: FnOnce() -> std::io::Result<Vec<SocketAddr>> + Send + 'static,
{
    let permit = ResolvePermit::acquire(in_flight)?;
    let (result_tx, result_rx) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("atm-peer-resolve".to_owned())
        .spawn(move || {
            let result = resolve();
            let _ = result_tx.send(result);
            drop(permit);
        })
        .map_err(|source| peer_delivery_failure("start configured peer resolver", source))?;

    match result_rx.recv_timeout(timeout) {
        Ok(Ok(addresses)) => Ok(addresses),
        Ok(Err(source)) => Err(peer_delivery_failure("resolve configured peer", source)),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(AtmError::remote_delivery_unconfirmed(
            "local persistence succeeded but configured peer DNS resolution exceeded the delivery deadline",
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(AtmError::remote_delivery_unconfirmed(
            "local persistence succeeded but configured peer resolver stopped before returning an address",
        )),
    }
}

#[derive(Debug)]
struct ResolvePermit {
    in_flight: Arc<AtomicUsize>,
}

impl ResolvePermit {
    fn acquire(in_flight: Arc<AtomicUsize>) -> Result<Self, AtmError> {
        let mut current = in_flight.load(Ordering::Acquire);
        loop {
            if current >= MAX_IN_FLIGHT_PEER_RESOLVES {
                return Err(AtmError::remote_delivery_unconfirmed(
                    "local persistence succeeded but configured peer DNS resolution capacity is exhausted",
                ));
            }
            match in_flight.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Ok(Self { in_flight }),
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for ResolvePermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

fn peer_response_budget(deadline: RequestDeadline) -> Result<Duration, AtmError> {
    deadline
        .remaining()
        .map(|remaining| remaining.min(PEER_HTTP_LOCAL_RESPONSE_BUDGET))
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| {
            AtmError::remote_delivery_unconfirmed(
                "local persistence succeeded but the peer response deadline expired before delivery completed",
            )
        })
}

pub(crate) fn ensure_matching_send_response(
    response: &ResponseEnvelope,
    write: &WriteRequest,
    endpoint: &PeerEndpoint,
) -> Result<(), AtmError> {
    let expected_message_id = write.origin_message_id.ok_or_else(|| {
        AtmError::remote_delivery_unconfirmed(
            "local persistence succeeded but peer delivery write had no immutable origin message ID",
        )
    })?;
    let actual_message_id = match response {
        ResponseEnvelope::Send(SendResponseEnvelope::Sent(outcome)) => outcome.message_id,
        ResponseEnvelope::Send(SendResponseEnvelope::Acknowledged(outcome)) => {
            let atm_core::ack::AckReplyDisposition::Sent {
                reply_message_id, ..
            } = &outcome.reply_disposition;
            *reply_message_id
        }
        ResponseEnvelope::Error(error) => {
            return Err(AtmError::remote_delivery_unconfirmed(format!(
                "local persistence succeeded but configured peer `{}` rejected the write: {error}",
                endpoint.canonical_host
            )));
        }
        response => {
            return Err(AtmError::remote_delivery_unconfirmed(format!(
                "local persistence succeeded but configured peer `{}` returned unexpected response `{response:?}`",
                endpoint.canonical_host
            )));
        }
    };
    if actual_message_id != expected_message_id {
        return Err(AtmError::remote_delivery_unconfirmed(format!(
            "local persistence succeeded but configured peer `{}` confirmed message `{actual_message_id}` instead of `{expected_message_id}`",
            endpoint.canonical_host
        )));
    }
    Ok(())
}

fn peer_delivery_failure(action: &str, source: std::io::Error) -> AtmError {
    AtmError::remote_delivery_unconfirmed(format!(
        "local persistence succeeded but failed to {action}: {source}"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::{MAX_IN_FLIGHT_PEER_RESOLVES, ResolvePermit, resolve_with_timeout};

    #[test]
    fn stalled_resolution_returns_at_the_delivery_deadline_and_releases_its_slot() {
        let in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let error = resolve_with_timeout(
            std::sync::Arc::clone(&in_flight),
            Duration::from_millis(10),
            move || {
                started_tx.send(()).expect("report resolution start");
                release_rx.recv().expect("release stalled resolution");
                Ok(Vec::new())
            },
        )
        .expect_err("a stalled resolver must not outlive the delivery deadline");

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("resolver started");
        assert!(
            error
                .to_string()
                .contains("DNS resolution exceeded the delivery deadline"),
            "unexpected error: {error}"
        );
        assert_eq!(in_flight.load(std::sync::atomic::Ordering::Acquire), 1);

        release_tx.send(()).expect("release resolver");
        for _ in 0..100 {
            if in_flight.load(std::sync::atomic::Ordering::Acquire) == 0 {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("completed resolver must release its in-flight slot");
    }

    #[test]
    fn resolver_capacity_fails_closed_without_a_queue() {
        let in_flight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let permits = (0..MAX_IN_FLIGHT_PEER_RESOLVES)
            .map(|_| ResolvePermit::acquire(std::sync::Arc::clone(&in_flight)))
            .collect::<Result<Vec<_>, _>>()
            .expect("all configured resolver slots are available");

        let error = ResolvePermit::acquire(in_flight).expect_err("capacity must fail closed");
        assert!(
            error
                .to_string()
                .contains("DNS resolution capacity is exhausted"),
            "unexpected error: {error}"
        );
        drop(permits);
    }
}
