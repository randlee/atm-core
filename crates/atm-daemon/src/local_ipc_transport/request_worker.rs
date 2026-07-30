use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use atm_core::api::{
    ApiRequest, ApiRouter, AuthenticatedIngress, HttpFrameReader, RequestDeadline, decode_request,
    write_local_http_response,
};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::{RequestId, ResponseEnvelope};
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use crate::SubsystemObservability;
use crate::active_connection_registry::{
    ActiveConnectionRegistry, DispatchReapSummary, TrackedDispatchHandle,
};

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{
    DISPATCH_PANIC_RECOVERED_MESSAGE, MAX_CONCURRENT_CONNECTIONS, MAX_KEEP_ALIVE_REQUESTS,
    REQUEST_DEADLINE, write_shutdown_response,
};

type DispatchResultRx = std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>;
type DispatchCompletionRx = std::sync::mpsc::Receiver<()>;
type DispatchWorkerHandle = std::thread::JoinHandle<()>;
type DispatchWorker = (DispatchResultRx, DispatchCompletionRx, DispatchWorkerHandle);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestExecutionRisk {
    ReadOnly,
    SideEffecting,
}

pub(super) fn handle_connection(
    mut stream: LocalSocketStream,
    dispatcher: Arc<dyn ApiRouter + Send + Sync>,
    force_shutdown: &AtomicBool,
    registry: Arc<ActiveConnectionRegistry>,
    observability: &SubsystemObservability,
) -> Result<(), AtmError> {
    apply_primary_request_deadline(&mut stream);
    let mut frames = HttpFrameReader::new();
    for request_count in 1..=MAX_KEEP_ALIVE_REQUESTS {
        if force_shutdown.load(Ordering::SeqCst) {
            return write_shutdown_response(&mut stream).map(|_| ());
        }
        let Some(raw_request) = read_bounded_http_request(&mut frames, &mut stream)? else {
            return Ok(());
        };
        let keep_alive = raw_request
            .header("connection")
            .is_some_and(|value| value.eq_ignore_ascii_case("keep-alive"))
            && request_count < MAX_KEEP_ALIVE_REQUESTS;
        let request = decode_request(raw_request)?;
        tracing::debug!(
            max_http_request_body_bytes = atm_core::MAX_HTTP_REQUEST_BODY_BYTES,
            "daemon HTTP request accepted under configured size cap"
        );
        let request_id = atm_core::protocol::next_request_id();
        // The deadline starts at local HTTP admission, not when a later dispatch
        // worker happens to run. It is propagated unchanged through peer delivery.
        let response = dispatch_request(
            request_id,
            request,
            RequestDeadline::after(REQUEST_DEADLINE),
            dispatcher.clone(),
            &registry,
            observability,
        )?;
        if let Err(error) = write_local_http_response(&mut stream, &response, keep_alive) {
            let classification = classify_connection_failure(&error);
            if classification == ConnectionFailureClassification::ExpectedPeerDisconnect {
                observability.emit_event_or_warn(
                    observability
                        .event(
                            "connection_worker",
                            classification.as_str(),
                            "same-host peer disconnected before the daemon HTTP response completed",
                        )
                        .with_connection_failure(DaemonConnectionFailureFields {
                            code: error.code(),
                            request_id: Some(request_id),
                            classification,
                        })
                        .with_transport_context("response_write"),
                );
                return Ok(());
            }
            emit_connection_failure_event(
                observability,
                &error,
                Some(request_id),
                "response_write",
            );
            return Err(error);
        }
        emit_dispatch_panic_recovery(observability, registry.reap_finished_dispatches()?);
        if !keep_alive {
            return Ok(());
        }
    }
    Ok(())
}

fn emit_dispatch_panic_recovery(
    observability: &SubsystemObservability,
    summary: DispatchReapSummary,
) {
    if summary.recovered_panics == 0 {
        return;
    }
    observability.emit_or_warn(
        "dispatch_worker",
        "panic_recovered",
        DISPATCH_PANIC_RECOVERED_MESSAGE,
    );
}

pub(super) fn classify_connection_failure(error: &AtmError) -> ConnectionFailureClassification {
    if error.code() == AtmErrorCode::MessageValidationFailed {
        return ConnectionFailureClassification::MalformedRequest;
    }
    let haystacks = [error.message().to_ascii_lowercase()];
    if haystacks.iter().any(|value| {
        value.contains("broken pipe")
            || value.contains("connection reset")
            || value.contains("connection aborted")
            || value.contains("unexpected eof")
            || value.contains("end of file")
    }) {
        return ConnectionFailureClassification::ExpectedPeerDisconnect;
    }
    if matches!(
        error.code(),
        AtmErrorCode::DaemonUnavailable | AtmErrorCode::WaitTimeout
    ) {
        return ConnectionFailureClassification::TransportFailure;
    }
    ConnectionFailureClassification::RequestFailure
}

pub(super) fn emit_connection_failure_event(
    observability: &SubsystemObservability,
    error: &AtmError,
    request_id: Option<RequestId>,
    transport_context: &'static str,
) {
    let classification = classify_connection_failure(error);
    observability.emit_event_or_warn(
        observability
            .event(
                "connection_worker",
                classification.as_str(),
                error.message().to_owned(),
            )
            .with_connection_failure(DaemonConnectionFailureFields {
                code: error.code(),
                request_id,
                classification,
            })
            .with_transport_context(transport_context),
    );
}

fn dispatch_request(
    request_id: RequestId,
    request: ApiRequest,
    deadline: RequestDeadline,
    dispatcher: Arc<dyn ApiRouter + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
    observability: &SubsystemObservability,
) -> Result<ResponseEnvelope, AtmError> {
    let execution_risk = request_execution_risk(&request);
    let result_rx = (|| {
        let (result_rx, completion_rx, dispatch_handle) =
            spawn_dispatch_worker(request, deadline, dispatcher, Arc::clone(registry))?;
        registry.push_dispatch_handle(
            TrackedDispatchHandle {
                completion_rx,
                join_handle: dispatch_handle,
            },
            MAX_CONCURRENT_CONNECTIONS,
        )?;
        Ok::<DispatchResultRx, AtmError>(result_rx)
    })()
    .inspect_err(|error| {
        emit_connection_failure_event(observability, error, Some(request_id), "dispatch_request");
    })?;
    Ok(await_dispatch_response(
        request_id,
        execution_risk,
        deadline,
        result_rx,
    ))
}

fn spawn_dispatch_worker(
    request: ApiRequest,
    deadline: RequestDeadline,
    dispatcher: Arc<dyn ApiRouter + Send + Sync>,
    dispatch_registry: Arc<ActiveConnectionRegistry>,
) -> Result<DispatchWorker, AtmError> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
    // The same-host dispatch worker is intentionally allowed to run to natural
    // completion after a caller-side timeout. Requests may have already crossed
    // durable or side-effecting boundaries, so forcing cancellation at
    // REQUEST_DEADLINE would create a more ambiguous contract than the current
    // "response timed out; work may still complete in the background" surface.
    // The completion channel plus tracked join handle keep that bounded worker
    // visible to shutdown/reap logic instead of leaking silently.
    let dispatch_handle = std::thread::Builder::new()
        .name("local-ipc-dispatch".to_string())
        .spawn(move || {
            let _dispatch_work = dispatch_registry.register_dispatch_work();
            let response = dispatcher
                .route(request, AuthenticatedIngress::Local, deadline)
                .map(|response| response.into_inner());
            let _ = result_tx.send(response);
            let _ = completion_tx.send(());
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC dispatch worker")
        })?;
    Ok((result_rx, completion_rx, dispatch_handle))
}

fn apply_primary_request_deadline(stream: &mut LocalSocketStream) {
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
}

fn read_bounded_http_request(
    frames: &mut HttpFrameReader,
    stream: &mut LocalSocketStream,
) -> Result<Option<atm_core::api::HttpRequest>, AtmError> {
    // `apply_primary_request_deadline` installs the same deadline directly on
    // this socket. Keeping the stateful reader on this worker avoids a thread
    // per request while preserving buffered keep-alive framing.
    frames.read_request(stream)
}

fn await_dispatch_response(
    request_id: RequestId,
    execution_risk: RequestExecutionRisk,
    deadline: RequestDeadline,
    result_rx: std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>,
) -> ResponseEnvelope {
    let remaining = deadline.remaining().unwrap_or(Duration::ZERO);
    match result_rx.recv_timeout(remaining) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => ResponseEnvelope::Error(error),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = "local_ipc",
                action = "dispatch",
                outcome = "deadline_exceeded",
                request_id = %request_id,
                deadline_ms = remaining.as_millis(),
                "daemon request dispatcher exceeded the runtime deadline"
            );
            dispatch_timeout_response(execution_risk)
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ResponseEnvelope::Error(AtmError::daemon_unavailable(
                "daemon request dispatcher stopped before returning a response",
            ))
        }
    }
}

fn request_execution_risk(request: &ApiRequest) -> RequestExecutionRisk {
    match request {
        ApiRequest::Messages(_) | ApiRequest::Doctor(_) | ApiRequest::CompatibilityPreflight(_) => {
            RequestExecutionRisk::ReadOnly
        }
        ApiRequest::Write(_)
        | ApiRequest::Heartbeat(_)
        | ApiRequest::Clear(_)
        | ApiRequest::PeerSync(_)
        | ApiRequest::ReloadRuntimeView => RequestExecutionRisk::SideEffecting,
    }
}

fn dispatch_timeout_response(execution_risk: RequestExecutionRisk) -> ResponseEnvelope {
    let error = match execution_risk {
        RequestExecutionRisk::ReadOnly => AtmError::daemon_unavailable(
            "daemon request exceeded the 3s runtime deadline; retry the read-only ATM command after the same-host daemon catches up",
        ),
        RequestExecutionRisk::SideEffecting => AtmError::daemon_may_have_executed(
            "daemon request exceeded the 3s runtime deadline after side-effecting work may have started",
        ),
    };
    ResponseEnvelope::Error(error)
}

#[cfg(test)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn install_injected_accept_error_for_test(
    runtime: &mut PreparedRuntimeServer,
    signal: std::sync::mpsc::SyncSender<()>,
) {
    runtime.accept_error_inject = Some(signal);
}

#[cfg(test)]
mod tests {
    use super::{
        RequestExecutionRisk, classify_connection_failure, dispatch_timeout_response,
        handle_connection, request_execution_risk,
    };
    use atm_core::api::ApiRequest;
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::observability::ConnectionFailureClassification;
    use atm_core::protocol::{PeerSyncRequest, RequestEnvelope, ResponseEnvelope};
    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::traits::Listener as _;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    use crate::active_connection_registry::ActiveConnectionRegistry;
    use crate::test_support::{DoctorOnlyDispatcher, connect_local_ipc_with_timeout};
    use crate::{DaemonSubsystem, SubsystemObservability};

    #[test]
    fn side_effecting_timeout_returns_may_have_executed_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::SideEffecting);
        let ResponseEnvelope::Error(error) = response else {
            panic!("expected error envelope");
        };
        assert_eq!(error.code(), AtmErrorCode::DaemonMayHaveExecuted);
    }

    #[test]
    fn read_only_timeout_returns_retryable_daemon_unavailable_code() {
        let response = dispatch_timeout_response(RequestExecutionRisk::ReadOnly);
        let ResponseEnvelope::Error(error) = response else {
            panic!("expected error envelope");
        };
        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
    }

    #[test]
    fn request_execution_risk_classifies_clear_as_side_effecting() {
        let tmp = std::env::temp_dir();
        let request = RequestEnvelope::Clear(ClearQuery {
            home_dir: tmp.clone(),
            current_dir: tmp,
            caller_identity: atm_core::test_support::TEST_SENDER.parse().expect("caller"),
            caller_team: atm_core::test_support::TEST_TEAM.parse().expect("team"),
            older_than: None,
            idle_only: false,
            dry_run: false,
        });
        assert_eq!(
            request_execution_risk(&ApiRequest::new(request)),
            RequestExecutionRisk::SideEffecting
        );
    }

    #[test]
    fn request_execution_risk_classifies_peer_sync_as_side_effecting() {
        let request = RequestEnvelope::PeerSync(PeerSyncRequest {
            peer: "peer.example.test".parse().expect("peer host"),
        });

        assert_eq!(
            request_execution_risk(&ApiRequest::new(request)),
            RequestExecutionRisk::SideEffecting
        );
    }

    #[test]
    fn request_execution_risk_classifies_doctor_as_read_only() {
        let tmp = std::env::temp_dir();
        let request = RequestEnvelope::Doctor(DoctorQuery {
            home_dir: tmp.clone(),
            current_dir: tmp,
            team_override: None,
            ..DoctorQuery::default()
        });
        assert_eq!(
            request_execution_risk(&ApiRequest::new(request)),
            RequestExecutionRisk::ReadOnly
        );
    }

    #[test]
    fn classify_connection_failure_marks_validation_errors_as_malformed_requests() {
        let error = AtmError::validation("bad frame");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::MalformedRequest
        );
    }

    #[test]
    fn classify_connection_failure_marks_disconnect_strings_as_expected_peer_disconnects() {
        let error = AtmError::daemon_unavailable("Broken pipe while writing response");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::ExpectedPeerDisconnect
        );
    }

    #[test]
    fn classify_connection_failure_marks_daemon_unavailable_as_transport_failure() {
        let error = AtmError::daemon_unavailable("socket timed out");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::TransportFailure
        );
    }

    #[test]
    fn classify_connection_failure_marks_other_errors_as_request_failure() {
        let error = AtmError::new(AtmErrorCode::MailboxReadFailed, "mailbox lookup failed");

        assert_eq!(
            classify_connection_failure(&error),
            ConnectionFailureClassification::RequestFailure
        );
    }

    #[test]
    fn wedged_peer_read_is_bounded_by_primary_request_deadline() {
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("wedged-peer.sock");
        #[cfg(unix)]
        if socket_path.exists() {
            std::fs::remove_file(&socket_path).expect("remove stale socket");
        }
        let socket_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
            .expect("socket name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(socket_name.clone())
            .create_sync()
            .expect("bind listener");
        let (client_connected_tx, client_connected_rx) = std::sync::mpsc::sync_channel(1);
        let (release_client_tx, release_client_rx) = std::sync::mpsc::sync_channel(1);
        let client = std::thread::spawn(move || {
            let _stream = connect_local_ipc_with_timeout(socket_name, Duration::from_secs(5))
                .expect("connect wedged peer");
            client_connected_tx.send(()).expect("signal connected");
            let _ = release_client_rx.recv_timeout(Duration::from_secs(5));
        });

        let server_stream = listener.accept().expect("accept");
        client_connected_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("client connected");
        let started = Instant::now();
        let result = handle_connection(
            server_stream,
            Arc::new(DoctorOnlyDispatcher),
            &AtomicBool::new(false),
            Arc::new(ActiveConnectionRegistry::default()),
            &SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
        );

        assert!(
            started.elapsed() < Duration::from_secs(5),
            "wedged same-host peer must not keep the connection worker blocked indefinitely"
        );
        assert!(
            result.is_err(),
            "a peer that never sends an HTTP request should time out"
        );
        let _ = release_client_tx.send(());
        client.join().expect("join wedged peer");
    }
}
