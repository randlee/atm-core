//! Outbound configured-peer delivery.
//!
//! This adapter owns only the remote connection setup and peer provenance
//! header. The request and response wire protocol is the canonical ATM HTTP
//! path shared with local CLI and graft clients.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use atm_core::api::{RequestDeadline, read_http_response, write_http_request_with_headers};
use atm_core::error::AtmError;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, SendResponseEnvelope};
use atm_core::send::WriteRequest;
use atm_storage::PeerEndpoint;

use crate::peer_http_listener::{
    PEER_HTTP_LOCAL_RESPONSE_BUDGET, PLAINTEXT_PEER_SOURCE_HOST_HEADER, PeerHttpRuntimeConfig,
};

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
    let _ = peer_response_budget(deadline)?;
    let addresses = (endpoint.canonical_host.as_str(), endpoint.port.get())
        .to_socket_addrs()
        .map_err(|source| peer_delivery_failure("resolve configured peer", source))?;
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
