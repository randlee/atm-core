use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use atm_core::api::{
    ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline, decode_request,
    read_http_request, write_http_response,
};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::{RequestEnvelope, RequestId, ResponseEnvelope};
use interprocess::local_socket::Stream as LocalSocketStream;

use crate::SubsystemObservability;
use crate::active_connection_registry::{
    ActiveConnectionRegistry, DispatchReapSummary, TrackedDispatchHandle,
};

#[cfg(test)]
use super::PreparedRuntimeServer;
use super::{
    DISPATCH_PANIC_RECOVERED_MESSAGE, MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE,
    write_shutdown_response,
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
    if force_shutdown.load(Ordering::SeqCst) {
        return write_shutdown_response(&mut stream).map(|_| ());
    }
    let request = match read_http_request(&mut stream)? {
        Some(request) => decode_request(request)?,
        None => return Ok(()),
    };
    tracing::debug!(
        max_http_request_body_bytes = atm_core::MAX_HTTP_REQUEST_BODY_BYTES,
        "daemon HTTP request accepted under configured size cap"
    );
    let request_id = atm_core::protocol::next_request_id();
    let response = dispatch_request(request_id, request, dispatcher, &registry, observability)?;
    if let Err(error) = write_http_response(&mut stream, &response) {
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
        emit_connection_failure_event(observability, &error, Some(request_id), "response_write");
        return Err(error);
    }
    emit_dispatch_panic_recovery(observability, registry.reap_finished_dispatches()?);
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
    request: RequestEnvelope,
    dispatcher: Arc<dyn ApiRouter + Send + Sync>,
    registry: &Arc<ActiveConnectionRegistry>,
    observability: &SubsystemObservability,
) -> Result<ResponseEnvelope, AtmError> {
    let execution_risk = request_execution_risk(&request);
    let result_rx = (|| {
        let (result_rx, completion_rx, dispatch_handle) =
            spawn_dispatch_worker(request, dispatcher, Arc::clone(registry))?;
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
        result_rx,
    ))
}

fn spawn_dispatch_worker(
    request: RequestEnvelope,
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
                .route(
                    ApiRequest::new(request),
                    AuthenticatedIngress::Local,
                    RequestDeadline::after(REQUEST_DEADLINE),
                )
                .map(|response| response.into_inner());
            let _ = result_tx.send(response);
            let _ = completion_tx.send(());
        })
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn daemon local IPC dispatch worker")
        })?;
    Ok((result_rx, completion_rx, dispatch_handle))
}

fn await_dispatch_response(
    request_id: RequestId,
    execution_risk: RequestExecutionRisk,
    result_rx: std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>,
) -> ResponseEnvelope {
    match result_rx.recv_timeout(REQUEST_DEADLINE) {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => ResponseEnvelope::Error(error),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::warn!(
                subsystem = "local_ipc",
                action = "dispatch",
                outcome = "deadline_exceeded",
                request_id = %request_id,
                deadline_ms = REQUEST_DEADLINE.as_millis(),
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

fn request_execution_risk(request: &RequestEnvelope) -> RequestExecutionRisk {
    match request {
        RequestEnvelope::List(_)
        | RequestEnvelope::Peek(_)
        | RequestEnvelope::Receive(_)
        | RequestEnvelope::Doctor(_) => RequestExecutionRisk::ReadOnly,
        RequestEnvelope::CompatibilityPreflight(_) => RequestExecutionRisk::ReadOnly,
        RequestEnvelope::Send(_) | RequestEnvelope::Heartbeat(_) | RequestEnvelope::Clear(_) => {
            RequestExecutionRisk::SideEffecting
        }
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
        request_execution_risk,
    };
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::observability::ConnectionFailureClassification;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};

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
            request_execution_risk(&request),
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
            request_execution_risk(&request),
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
}
