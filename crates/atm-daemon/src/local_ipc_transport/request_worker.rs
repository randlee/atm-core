use std::sync::Arc;
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use atm_core::api::{ApiRequest, ApiRouter, AuthenticatedIngress, RequestDeadline, decode_request};
#[cfg(unix)]
use atm_core::api::{HttpFrameReader, write_local_http_response};
use atm_core::error::AtmError;
use atm_core::error_codes::AtmErrorCode;
use atm_core::observability::{ConnectionFailureClassification, DaemonConnectionFailureFields};
use atm_core::protocol::{RequestId, ResponseEnvelope};
#[cfg(unix)]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::traits::Stream as _;

use crate::SubsystemObservability;
use crate::active_connection_registry::ActiveConnectionRegistry;
use crate::daemon_worker_join::{
    CompletionTrackedJoinHandle, JoinTimeoutPolicy, LOCAL_WORKER_JOIN_DEADLINE, join_with_timeout,
};
use crate::local_admission::{BOUNDED_ADMISSION_RETRY_INTERVAL, send_with_bounded_admission};

use crate::MAX_KEEP_ALIVE_REQUESTS;
#[cfg(all(test, unix))]
use crate::local_ipc_transport::PreparedRuntimeServer;
#[cfg(unix)]
use crate::local_ipc_transport::shutdown::reject_shutdown_request;

const REQUEST_DEADLINE: Duration = Duration::from_secs(3);
const LOCAL_IPC_DISPATCH_JOIN_POLICY: JoinTimeoutPolicy = JoinTimeoutPolicy {
    subsystem: "local_ipc_transport",
    worker_kind: "local IPC dispatch worker",
    panic_message: "local IPC dispatch worker panicked during shutdown",
    timeout_message: "local IPC dispatch worker exceeded the shutdown join deadline",
};
pub(crate) const DISPATCH_PANIC_RECOVERED_MESSAGE: &str =
    "daemon local IPC dispatch worker panicked before completing; transport thread recovered";

type DispatchResultRx = std::sync::mpsc::Receiver<Result<ResponseEnvelope, AtmError>>;
pub(crate) const MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS: usize = 8;

struct DispatchWork {
    request: ApiRequest,
    deadline: RequestDeadline,
    result_tx: std::sync::mpsc::SyncSender<Result<ResponseEnvelope, AtmError>>,
}

/// Fixed dispatcher workers with one bounded worker-batch queue preserve the
/// post-timeout execution contract without creating a thread per admission.
///
/// A queued frame has crossed only the in-memory dispatch boundary.  When the
/// queue is full, admission remains deadline-bounded rather than adding an
/// unbounded daemon-side backlog.
pub(crate) struct DispatchWorkerPool {
    sender: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<DispatchWork>>>,
    workers: std::sync::Mutex<Vec<CompletionTrackedJoinHandle<()>>>,
    #[cfg(test)]
    shutdown_join_signal: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

impl DispatchWorkerPool {
    pub(crate) fn start(
        dispatcher: Arc<dyn ApiRouter + Send + Sync>,
        registry: Arc<ActiveConnectionRegistry>,
        observability: SubsystemObservability,
        worker_count: usize,
    ) -> Result<Arc<Self>, AtmError> {
        let (sender, receiver) = std::sync::mpsc::sync_channel::<DispatchWork>(worker_count);
        let receiver = Arc::new(std::sync::Mutex::new(receiver));
        let mut workers = Vec::with_capacity(worker_count);
        for worker_index in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            let dispatcher = Arc::clone(&dispatcher);
            let registry = Arc::clone(&registry);
            let observability = observability.clone();
            let (completion_tx, completion_rx) = std::sync::mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name(format!("local-ipc-dispatch-{worker_index}"))
                .spawn(move || {
                    let _completion_tx = completion_tx;
                    loop {
                    let work = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => return,
                    };
                    let Ok(work) = work else {
                        return;
                    };
                    let _dispatch_work = registry.register_dispatch_work();
                    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dispatcher
                            .route(work.request, AuthenticatedIngress::Local, work.deadline)
                            .map(|response| response.into_inner())
                    }));
                    let response = match response {
                        Ok(response) => response,
                        Err(_) => {
                            observability.emit_or_warn(
                                "dispatch_worker",
                                "panic_recovered",
                                DISPATCH_PANIC_RECOVERED_MESSAGE,
                            );
                            Err(AtmError::daemon_unavailable(
                                "daemon request dispatcher panicked before returning a response",
                            ))
                        }
                    };
                    let _ = work.result_tx.send(response);
                    }
                })
                .map_err(|_| AtmError::daemon_unavailable("failed to start local IPC dispatch worker"))?;
            workers.push(CompletionTrackedJoinHandle {
                completion_rx,
                join_handle: worker,
            });
        }
        Ok(Arc::new(Self {
            sender: std::sync::Mutex::new(Some(sender)),
            workers: std::sync::Mutex::new(workers),
            #[cfg(test)]
            shutdown_join_signal: std::sync::Mutex::new(None),
        }))
    }

    fn dispatch(
        &self,
        request: ApiRequest,
        deadline: RequestDeadline,
    ) -> Result<DispatchResultRx, AtmError> {
        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let sender = self
            .sender
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("local IPC dispatch sender lock poisoned"))?
            .clone()
            .ok_or_else(|| {
                AtmError::daemon_unavailable("local IPC dispatch workers are stopping")
            })?;
        let work = DispatchWork {
            request,
            deadline,
            result_tx,
        };
        send_with_bounded_admission(
            &sender,
            work,
            || {
                let Some(remaining) = deadline.remaining() else {
                    return Err(AtmError::daemon_unavailable(
                        "local IPC dispatch capacity remained saturated until the request deadline; retry the command after the daemon catches up",
                    ));
                };
                Ok(remaining.min(BOUNDED_ADMISSION_RETRY_INTERVAL))
            },
            "local IPC dispatch workers stopped accepting work",
        )?;
        Ok(result_rx)
    }

    #[cfg(test)]
    pub(crate) fn install_shutdown_join_signal_for_test(
        &self,
        signal: std::sync::mpsc::SyncSender<()>,
    ) {
        *self
            .shutdown_join_signal
            .lock()
            .expect("lock dispatch-worker shutdown-join test signal") = Some(signal);
    }

    #[cfg(test)]
    fn signal_shutdown_join_for_test(&self) {
        if let Some(signal) = self
            .shutdown_join_signal
            .lock()
            .expect("lock dispatch-worker shutdown-join test signal")
            .take()
        {
            let _ = signal.send(());
        }
    }

    pub(crate) fn shutdown(&self) -> Result<(), AtmError> {
        self.sender
            .lock()
            .map_err(|_| AtmError::daemon_unavailable("local IPC dispatch sender lock poisoned"))?
            .take();
        let workers = std::mem::take(&mut *self.workers.lock().map_err(|_| {
            AtmError::daemon_unavailable("local IPC dispatch worker lock poisoned")
        })?);
        #[cfg(test)]
        self.signal_shutdown_join_for_test();
        for worker in workers {
            join_with_timeout(
                worker,
                LOCAL_WORKER_JOIN_DEADLINE,
                LOCAL_IPC_DISPATCH_JOIN_POLICY,
            )?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequestExecutionRisk {
    ReadOnly,
    SideEffecting,
}

#[cfg(unix)]
pub(crate) fn handle_connection(
    mut stream: LocalSocketStream,
    force_shutdown: &AtomicBool,
    dispatch_workers: &DispatchWorkerPool,
    observability: &SubsystemObservability,
) -> Result<(), AtmError> {
    apply_primary_request_deadline(&mut stream);
    let mut frames = HttpFrameReader::new();
    let mut request_count = 0;
    loop {
        if force_shutdown.load(Ordering::SeqCst) {
            return reject_shutdown_request(&mut stream);
        }
        let Some(raw_request) = read_bounded_http_request(&mut frames, &mut stream)? else {
            return Ok(());
        };
        request_count += 1;
        let pending = enqueue_buffered_requests(
            raw_request,
            &mut request_count,
            &mut frames,
            dispatch_workers,
            observability,
        )?;
        if !write_pending_responses(&mut stream, pending, observability)? {
            return Ok(());
        }
    }
}

#[cfg(unix)]
fn enqueue_buffered_requests(
    raw_request: atm_core::api::HttpRequest,
    request_count: &mut usize,
    frames: &mut HttpFrameReader,
    dispatch_workers: &DispatchWorkerPool,
    observability: &SubsystemObservability,
) -> Result<Vec<PendingRequest>, AtmError> {
    let mut pending = vec![enqueue_request(
        raw_request,
        *request_count,
        dispatch_workers,
        observability,
    )?];
    while pending.len() < MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS
        && pending.last().is_some_and(|entry| entry.keep_alive)
        && *request_count < MAX_KEEP_ALIVE_REQUESTS
    {
        let Some(raw_request) = frames.read_buffered_request()? else {
            break;
        };
        *request_count += 1;
        pending.push(enqueue_request(
            raw_request,
            *request_count,
            dispatch_workers,
            observability,
        )?);
    }
    Ok(pending)
}

#[cfg(unix)]
fn write_pending_responses(
    stream: &mut LocalSocketStream,
    pending: Vec<PendingRequest>,
    observability: &SubsystemObservability,
) -> Result<bool, AtmError> {
    for pending in pending {
        let response = await_dispatch_response(
            pending.request_id,
            pending.execution_risk,
            pending.deadline,
            pending.result_rx,
        );
        if let Err(error) = write_local_http_response(stream, &response, pending.keep_alive) {
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
                            request_id: Some(pending.request_id),
                            classification,
                        })
                        .with_transport_context("response_write"),
                );
                return Ok(false);
            }
            emit_connection_failure_event(
                observability,
                &error,
                Some(pending.request_id),
                "response_write",
            );
            return Err(error);
        }
        if !pending.keep_alive {
            return Ok(false);
        }
    }
    Ok(true)
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

pub(crate) struct PendingRequest {
    keep_alive: bool,
    request_id: RequestId,
    execution_risk: RequestExecutionRisk,
    deadline: RequestDeadline,
    result_rx: DispatchResultRx,
}

impl PendingRequest {
    pub(crate) fn keep_alive(&self) -> bool {
        self.keep_alive
    }

    pub(crate) fn await_response(self) -> ResponseEnvelope {
        await_dispatch_response(
            self.request_id,
            self.execution_risk,
            self.deadline,
            self.result_rx,
        )
    }
}

pub(crate) fn enqueue_request(
    raw_request: atm_core::api::HttpRequest,
    request_count: usize,
    dispatch_workers: &DispatchWorkerPool,
    observability: &SubsystemObservability,
) -> Result<PendingRequest, AtmError> {
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
    let deadline = RequestDeadline::after(REQUEST_DEADLINE);
    let execution_risk = request_execution_risk(&request);
    let result_rx = dispatch_workers
        .dispatch(request, deadline)
        .inspect_err(|error| {
            emit_connection_failure_event(
                observability,
                error,
                Some(request_id),
                "dispatch_request",
            );
        })?;
    Ok(PendingRequest {
        keep_alive,
        request_id,
        execution_risk,
        deadline,
        result_rx,
    })
}

#[cfg(unix)]
fn apply_primary_request_deadline(stream: &mut LocalSocketStream) {
    let _ = stream.set_recv_timeout(Some(REQUEST_DEADLINE));
    let _ = stream.set_send_timeout(Some(REQUEST_DEADLINE));
}

#[cfg(unix)]
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

#[cfg(all(test, unix))]
pub(crate) fn install_injected_accept_error_for_test(
    runtime: &mut PreparedRuntimeServer,
    signal: std::sync::mpsc::SyncSender<()>,
) {
    runtime.install_accept_error_injection_for_test(signal);
}

#[cfg(all(test, unix))]
mod tests {
    use super::{
        DispatchWorkerPool, MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS, RequestExecutionRisk,
        classify_connection_failure, dispatch_timeout_response, handle_connection,
        request_execution_risk,
    };
    use atm_core::api::{
        ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, HttpFrameReader, RequestDeadline,
        read_http_response, read_http_response_with_frame_reader, write_http_request,
    };
    use atm_core::clear::ClearQuery;
    use atm_core::doctor::DoctorQuery;
    use atm_core::error::AtmError;
    use atm_core::error_codes::AtmErrorCode;
    use atm_core::observability::ConnectionFailureClassification;
    use atm_core::protocol::{PeerSyncRequest, RequestEnvelope, ResponseEnvelope};
    use interprocess::local_socket::ListenerOptions;
    use interprocess::local_socket::traits::Listener as _;
    use std::io::{Read as _, Write as _};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    use crate::active_connection_registry::ActiveConnectionRegistry;
    use crate::test_support::{DoctorOnlyDispatcher, connect_local_ipc_with_timeout};
    use crate::{DaemonSubsystem, MAX_KEEP_ALIVE_REQUESTS, SubsystemObservability};

    #[derive(Debug)]
    struct BlockingDoctorDispatcher {
        started: std::sync::mpsc::SyncSender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl atm_core::boundary::sealed::Sealed for BlockingDoctorDispatcher {}

    impl ApiRouter for BlockingDoctorDispatcher {
        fn route(
            &self,
            _request: ApiRequest,
            _ingress: AuthenticatedIngress,
            _deadline: RequestDeadline,
        ) -> Result<ApiResponse, AtmError> {
            self.started.send(()).expect("signal occupied dispatcher");
            self.release
                .lock()
                .expect("release receiver")
                .recv()
                .expect("release occupied dispatcher");
            Ok(ApiResponse::new(ResponseEnvelope::Error(
                AtmError::daemon_unavailable("test dispatcher released"),
            )))
        }
    }

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
    fn saturated_dispatch_admission_stops_at_the_caller_deadline() {
        let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let dispatch_workers = DispatchWorkerPool::start(
            Arc::new(BlockingDoctorDispatcher {
                started: started_tx,
                release: Mutex::new(release_rx),
            }),
            registry,
            SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            1,
        )
        .expect("start occupied-dispatch fixture");
        let first = dispatch_workers
            .dispatch(
                ApiRequest::new(RequestEnvelope::Doctor(DoctorQuery::default())),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .expect("occupy the only dispatch worker");
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("dispatcher started first request");

        let queued = dispatch_workers
            .dispatch(
                ApiRequest::new(RequestEnvelope::Doctor(DoctorQuery::default())),
                RequestDeadline::after(Duration::from_secs(1)),
            )
            .expect("queue one request behind the occupied worker");
        let error = dispatch_workers
            .dispatch(
                ApiRequest::new(RequestEnvelope::Doctor(DoctorQuery::default())),
                RequestDeadline::after(Duration::ZERO),
            )
            .expect_err("a saturated bounded pool must honor its admission deadline");

        assert_eq!(error.code(), AtmErrorCode::DaemonUnavailable);
        assert!(error.message().contains("capacity remained saturated"));
        release_tx.send(()).expect("release occupied dispatcher");
        let _ = first
            .recv_timeout(Duration::from_secs(1))
            .expect("released dispatcher responds");
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("queued request starts after the first worker release");
        release_tx.send(()).expect("release queued dispatcher");
        let _ = queued
            .recv_timeout(Duration::from_secs(1))
            .expect("queued dispatcher responds after release");
        dispatch_workers
            .shutdown()
            .expect("saturated fixture shutdown completes after forced release");
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
        let registry = Arc::new(ActiveConnectionRegistry::default());
        let dispatch_workers = DispatchWorkerPool::start(
            Arc::new(DoctorOnlyDispatcher),
            Arc::clone(&registry),
            SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            1,
        )
        .expect("dispatch workers");
        let result = handle_connection(
            server_stream,
            &AtomicBool::new(false),
            dispatch_workers.as_ref(),
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
        dispatch_workers
            .shutdown()
            .expect("shutdown dispatch workers");
    }

    #[test]
    fn uds_keep_alive_serves_configured_counts_and_closes_at_bound() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        for count in [1_usize, 2, 8, 16, MAX_KEEP_ALIVE_REQUESTS] {
            let tempdir = TempDir::new().expect("tempdir");
            let socket_path = tempdir.path().join("keep-alive.sock");
            let socket_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
                .expect("socket name")
                .into_owned();
            let listener = ListenerOptions::new()
                .name(socket_name.clone())
                .create_sync()
                .expect("bind listener");
            let server = std::thread::spawn(move || {
                let stream = listener.accept().expect("accept client");
                let registry = Arc::new(ActiveConnectionRegistry::default());
                let dispatch_workers = DispatchWorkerPool::start(
                    Arc::new(DoctorOnlyDispatcher),
                    Arc::clone(&registry),
                    SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
                    1,
                )
                .expect("dispatch workers");
                handle_connection(
                    stream,
                    &AtomicBool::new(false),
                    dispatch_workers.as_ref(),
                    &SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
                )
                .expect("serve keep-alive requests");
                dispatch_workers
                    .shutdown()
                    .expect("shutdown dispatch workers");
            });
            let mut stream = connect_local_ipc_with_timeout(socket_name, Duration::from_secs(5))
                .expect("connect local IPC");

            for request_count in 1..=count {
                let mut wire = Vec::new();
                write_http_request(&mut wire, &request).expect("encode request");
                let connection = if request_count == count && count < MAX_KEEP_ALIVE_REQUESTS {
                    "close"
                } else {
                    "keep-alive"
                };
                let wire = String::from_utf8(wire)
                    .expect("request is UTF-8")
                    .replace("Connection: close", &format!("Connection: {connection}"));
                stream.write_all(wire.as_bytes()).expect("write request");
                stream.flush().expect("flush request");
                let response = read_http_response(&mut stream, &request).expect("read response");
                assert!(
                    matches!(response, ResponseEnvelope::Doctor(_)),
                    "keep-alive request {request_count} of {count} returned {response:?}"
                );
            }
            if count == MAX_KEEP_ALIVE_REQUESTS {
                server.join().expect("server join at keep-alive bound");
                let mut trailing = [0_u8; 1];
                assert_eq!(
                    stream
                        .read(&mut trailing)
                        .expect("read capped socket closure"),
                    0,
                    "the server must close after the configured keep-alive request bound"
                );
            } else {
                server.join().expect("server join");
            }
        }
    }

    #[test]
    fn uds_keep_alive_accepts_a_bounded_pipelined_run_in_response_order() {
        let request = RequestEnvelope::Doctor(DoctorQuery::default());
        let tempdir = TempDir::new().expect("tempdir");
        let socket_path = tempdir.path().join("pipelined.sock");
        let socket_name = atm_core::protocol::daemon_local_ipc_name_from_path(&socket_path)
            .expect("socket name")
            .into_owned();
        let listener = ListenerOptions::new()
            .name(socket_name.clone())
            .create_sync()
            .expect("bind listener");
        let server = std::thread::spawn(move || {
            let stream = listener.accept().expect("accept client");
            let registry = Arc::new(ActiveConnectionRegistry::default());
            let dispatch_workers = DispatchWorkerPool::start(
                Arc::new(DoctorOnlyDispatcher),
                Arc::clone(&registry),
                SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
                MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS,
            )
            .expect("dispatch workers");
            handle_connection(
                stream,
                &AtomicBool::new(false),
                dispatch_workers.as_ref(),
                &SubsystemObservability::disabled(DaemonSubsystem::LocalIpcTransport),
            )
            .expect("serve pipelined requests");
            dispatch_workers
                .shutdown()
                .expect("shutdown dispatch workers");
        });
        let mut stream = connect_local_ipc_with_timeout(socket_name, Duration::from_secs(5))
            .expect("connect local IPC");
        let mut wire = Vec::new();
        for request_count in 1..=MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS {
            let mut frame = Vec::new();
            write_http_request(&mut frame, &request).expect("encode request");
            let connection = if request_count == MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS {
                "close"
            } else {
                "keep-alive"
            };
            wire.extend_from_slice(
                String::from_utf8(frame)
                    .expect("request is UTF-8")
                    .replace("Connection: close", &format!("Connection: {connection}"))
                    .as_bytes(),
            );
        }
        stream.write_all(&wire).expect("write pipelined requests");
        stream.flush().expect("flush pipelined requests");
        let mut responses = HttpFrameReader::new();
        for request_count in 1..=MAX_IN_FLIGHT_KEEP_ALIVE_REQUESTS {
            let response =
                read_http_response_with_frame_reader(&mut responses, &mut stream, &request)
                    .expect("read response");
            assert!(
                matches!(response, ResponseEnvelope::Doctor(_)),
                "pipelined request {request_count} returned {response:?}"
            );
        }
        server.join().expect("server join");
    }
}
