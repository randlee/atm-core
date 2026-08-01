use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::thread;

use atm_core::api::ApiRouter;
use atm_core::api::{HttpFrameReader, write_http_response};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::ResponseEnvelope;
use interprocess::local_socket::Stream as LocalSocketStream;
use interprocess::local_socket::traits::Stream as _;

use super::{
    AcceptLoopOutcome, ActiveConnectionRegistry, CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
    LifecycleControlSourceAdapter, MAX_CONCURRENT_CONNECTIONS, REQUEST_DEADLINE, ServeLoopSignals,
    ShutdownBeacon, ShutdownResponseOutcome, SubsystemObservability,
    TERMINATE_REJECTION_GRACE_DEADLINE, handle_connection, record_serve_error,
    record_shutdown_signal, schedule_delayed_listener_wake, wake_listener, write_shutdown_response,
};

pub(super) fn take_accept_error(
    signals: &ServeLoopSignals,
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
) -> Result<Option<AtmError>, AtmError> {
    match signals.take_accept_error()? {
        Some(error) => Ok(Some(record_serve_error(
            lifecycle_control,
            shutdown_beacon,
            error,
        ))),
        None => Ok(None),
    }
}

pub(super) fn handle_shutdown_probe(
    stream: &mut LocalSocketStream,
    lifecycle_control: &LifecycleControlSourceAdapter,
    shutdown_beacon: &ShutdownBeacon,
    endpoint_path: &Path,
    terminate_probe_pending: &mut bool,
) -> Result<AcceptLoopOutcome, AtmError> {
    record_shutdown_signal(lifecycle_control, shutdown_beacon);
    match write_shutdown_response(stream)? {
        ShutdownResponseOutcome::RejectedRequest => Ok(AcceptLoopOutcome::Break(None)),
        ShutdownResponseOutcome::NoFrame if *terminate_probe_pending => {
            Ok(AcceptLoopOutcome::Break(None))
        }
        ShutdownResponseOutcome::NoFrame => {
            *terminate_probe_pending = true;
            if let Err(error) = schedule_delayed_listener_wake(
                endpoint_path.to_path_buf(),
                TERMINATE_REJECTION_GRACE_DEADLINE,
            ) {
                tracing::warn!(
                    subsystem = "local_ipc_transport",
                    action = "shutdown_probe_wake",
                    outcome = "failed",
                    deadline_ms = TERMINATE_REJECTION_GRACE_DEADLINE.as_millis(),
                    path = %endpoint_path.display(),
                    %error,
                    "failed to schedule delayed listener wake during shutdown probe"
                );
                let _ = wake_listener(endpoint_path);
                return Ok(AcceptLoopOutcome::Break(None));
            }
            Ok(AcceptLoopOutcome::Continue)
        }
    }
}

pub(super) fn reject_connection_when_capped(
    stream: &mut LocalSocketStream,
    active_connections: usize,
    observability: &SubsystemObservability,
) -> Result<bool, AtmError> {
    if active_connections < MAX_CONCURRENT_CONNECTIONS {
        return Ok(false);
    }
    observability.emit_event_or_warn(
        observability
            .event(
                "connection_admission",
                "saturated",
                format!(
                    "daemon rejected a same-host connection because the {}-connection cap was already exhausted",
                    MAX_CONCURRENT_CONNECTIONS
                ),
            )
            .with_connection_failure(DaemonConnectionFailureFields {
                code: AtmErrorCode::DaemonConnectionSaturated,
                request_id: None,
                classification: ConnectionFailureClassification::TransportFailure,
            })
            .with_transport_context("connection_cap")
            .with_extra_string_field("active_connections", active_connections.to_string()),
    );
    let response = ResponseEnvelope::Error(AtmError::new(
        AtmErrorCode::DaemonConnectionSaturated,
        format!("daemon connection cap exceeded ({active_connections} active connections)"),
    ));
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
    let has_request = match HttpFrameReader::new().read_request(stream) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(error) => {
            tracing::debug!(
                subsystem = "local_ipc_transport",
                action = "connection_cap_rejection",
                outcome = "request_frame_unavailable",
                %error,
                "connection-cap rejection could not read a request frame to correlate the error response"
            );
            false
        }
    };
    if has_request {
        let _ = write_http_response(stream, &response);
    }
    Ok(true)
}

pub(super) fn spawn_connection_worker<'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
    stream: LocalSocketStream,
    dispatcher: &Arc<dyn ApiRouter + Send + Sync>,
    force_shutdown: &Arc<AtomicBool>,
    registry: &Arc<ActiveConnectionRegistry>,
    observability: &SubsystemObservability,
) -> Result<(), AtmError> {
    let active = registry.register();
    let dispatcher = Arc::clone(dispatcher);
    let force_shutdown = Arc::clone(force_shutdown);
    let registry = Arc::clone(registry);
    let observability = observability.clone();
    thread::Builder::new()
        .name("local-ipc-connection-worker".to_string())
        .spawn_scoped(scope, move || {
            let _active = active;
            let result = catch_unwind(AssertUnwindSafe(|| {
                handle_connection(
                    stream,
                    dispatcher,
                    force_shutdown.as_ref(),
                    registry,
                    &observability,
                )
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    #[cfg(test)]
                    eprintln!("daemon local IPC connection handling failed: {error}");
                    tracing::warn!(
                        subsystem = "local_ipc_transport",
                        action = "connection_worker",
                        outcome = "classified_failure",
                        %error,
                        "daemon local IPC connection handling failed"
                    );
                }
                Err(_) => {
                    #[cfg(test)]
                    eprintln!("{CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE}");
                    observability.emit_or_warn(
                        "connection_worker",
                        "panic",
                        CONNECTION_WORKER_PANIC_RECOVERED_MESSAGE,
                    );
                    tracing::warn!(
                        subsystem = "local_ipc_transport",
                        action = "connection_worker",
                        outcome = "panic",
                        "daemon local IPC connection worker panicked; the transport thread recovered and continued shutdown accounting"
                    );
                }
            }
        })
        .map(|_| ())
        .map_err(|_source| {
            AtmError::daemon_unavailable("failed to spawn local IPC connection worker")


        })
}

#[cfg(test)]
mod tests {
    use super::reject_connection_when_capped;
    use crate::test_observability::TestDaemonObservability;
    use crate::test_support::connect_local_ipc_with_timeout;
    use crate::{DaemonSubsystem, SubsystemObservability};
    use atm_core::api::{read_http_response, write_http_request};
    use atm_core::doctor::DoctorQuery;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope, daemon_local_ipc_name_from_path};
    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::traits::Listener as _;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn capped_rejection_uses_the_actual_request_id() {
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("cap-rejection.sock");
        #[cfg(unix)]
        if socket_path.exists() {
            std::fs::remove_file(&socket_path).expect("remove stale socket");
        }
        let socket_name = daemon_local_ipc_name_from_path(&socket_path)
            .expect("socket name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(socket_name.clone())
            .create_sync()
            .expect("bind listener");
        let client_tempdir = tempdir.path().to_path_buf();
        let client = std::thread::spawn(move || {
            let mut stream = connect_local_ipc_with_timeout(socket_name, Duration::from_secs(5))
                .expect("connect");
            let request = RequestEnvelope::Doctor(DoctorQuery {
                home_dir: client_tempdir.join("home"),
                current_dir: client_tempdir.join("cwd"),
                team_override: None,
                ..DoctorQuery::default()
            });
            write_http_request(&mut stream, &request).expect("write request");
            read_http_response(&mut stream, &request).expect("read response")
        });

        let mut server_stream = listener.accept().expect("accept");
        let observability = Arc::new(
            TestDaemonObservability::new(tempdir.path().join("logs")).expect("observability"),
        );
        let subsystem =
            SubsystemObservability::new(DaemonSubsystem::LocalIpcTransport, observability);
        assert!(reject_connection_when_capped(&mut server_stream, 64, &subsystem).expect("reject"));

        let response = client.join().expect("join client");
        match response {
            ResponseEnvelope::Error(error) => {
                assert_eq!(error.code(), AtmErrorCode::DaemonConnectionSaturated);
                assert!(error.message().contains("connection cap exceeded"));
            }
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
