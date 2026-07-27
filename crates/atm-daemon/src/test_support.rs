use atm_core::api::{ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_core::doctor::{DoctorEnvironmentVisibility, DoctorReport, DoctorStatus, DoctorSummary};
#[cfg(test)]
use atm_core::error::AtmError;
use atm_core::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_runtime::RuntimeAssembly;

#[cfg(not(windows))]
use interprocess::local_socket::Name as LocalSocketName;
#[cfg(not(windows))]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(not(windows))]
use interprocess::local_socket::traits::Stream as _;
#[cfg(windows)]
use std::net::TcpStream as LocalSocketStream;

use crate::lifecycle_control::LifecycleControlSourceAdapter;

#[cfg(test)]
pub(crate) fn test_ack_write_request(
    home_dir: std::path::PathBuf,
    current_dir: std::path::PathBuf,
    caller_identity: atm_core::types::AgentName,
    caller_team: atm_core::types::TeamName,
    message_id: atm_core::schema::AtmMessageId,
    reply_body: &str,
) -> atm_core::send::WriteRequest {
    atm_core::ack::AckRequest {
        home_dir,
        current_dir,
        caller_identity,
        caller_chat_id: None,
        caller_team,
        message_id,
        reply_body: reply_body.to_string(),
    }
    .into_write_request()
}
#[cfg(not(windows))]
const TEST_LOCAL_IPC_CONNECT_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);
const TEST_LOCAL_IPC_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(not(windows))]
const TEST_LOCAL_IPC_CONNECT_RETRY_INITIAL_DELAY: std::time::Duration =
    std::time::Duration::from_millis(1);
#[cfg(not(windows))]
const TEST_LOCAL_IPC_CONNECT_RETRY_MAX_DELAY: std::time::Duration =
    std::time::Duration::from_millis(25);

pub(crate) struct LifecycleFlagResetGuard {
    lifecycle: LifecycleControlSourceAdapter,
}

impl LifecycleFlagResetGuard {
    pub(crate) fn install(lifecycle: LifecycleControlSourceAdapter) -> Self {
        lifecycle.set_terminate_for_test(false);
        lifecycle.set_reload_for_test(false);
        Self { lifecycle }
    }
}

impl Drop for LifecycleFlagResetGuard {
    fn drop(&mut self) {
        self.lifecycle.set_terminate_for_test(false);
        self.lifecycle.set_reload_for_test(false);
        if let Err(error) = self.lifecycle.reset_shared_state_for_test() {
            tracing::warn!(
                subsystem = "test_support",
                action = "reset_shared_state_for_test",
                outcome = "drain_failed",
                %error,
                "failed to drain shared lifecycle worker during test reset"
            );
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DoctorOnlyDispatcher;

impl atm_core::boundary::sealed::Sealed for DoctorOnlyDispatcher {}

impl ApiRouter for DoctorOnlyDispatcher {
    fn route(
        &self,
        request: ApiRequest,
        _ingress: AuthenticatedIngress,
        _deadline: RequestDeadline,
    ) -> Result<ApiResponse, atm_core::error::AtmError> {
        match request.into_inner() {
            RequestEnvelope::Doctor(_) => Ok(ApiResponse::new(ResponseEnvelope::Doctor(Box::new(
                DoctorReport {
                    summary: DoctorSummary {
                        status: DoctorStatus::Healthy,
                        message: "ok".to_string(),
                        info_count: 0,
                        warning_count: 0,
                        error_count: 0,
                    },
                    findings: Vec::new(),
                    recommendations: Vec::new(),
                    environment: DoctorEnvironmentVisibility {
                        atm_home: None,
                        atm_team: None,
                        atm_identity: None,
                        team_override: None,
                    },
                    client_context: atm_core::doctor::DoctorExecutionContext::default(),
                    daemon_context: None,
                    member_roster: None,
                    observability: AtmObservabilityHealth {
                        active_log_path: None,
                        logging_state: AtmObservabilityHealthState::Healthy,
                        query_state: Some(AtmObservabilityHealthState::Healthy),
                        maintenance: None,
                        diagnostic: None,
                        detail: None,
                    },
                    post_send: atm_core::doctor::PostSendDoctorReport::default(),
                    config: atm_core::boundary::ConfigDoctorReport::default(),
                    mail_store: atm_core::boundary::MailStoreDoctorReport::default(),
                    roster_store: atm_core::boundary::RosterStoreDoctorReport::default(),
                    daemon_runtime: None,
                    drift_findings: Vec::new(),
                    runtime_status: None,
                    bootstrap_trace: None,
                },
            )))),
            other => panic!("unexpected request in DoctorOnlyDispatcher: {other:?}"),
        }
    }
}

#[cfg(all(test, not(windows)))]
struct PanicUnwindSignal(Option<std::sync::mpsc::SyncSender<()>>);

#[cfg(all(test, not(windows)))]
impl Drop for PanicUnwindSignal {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[cfg(all(test, not(windows)))]
#[derive(Debug)]
pub(crate) struct PanicDispatcherWithUnwindSignal {
    unwind_tx: std::sync::Mutex<Option<std::sync::mpsc::SyncSender<()>>>,
}

#[cfg(all(test, not(windows)))]
impl PanicDispatcherWithUnwindSignal {
    pub(crate) fn new(unwind_tx: std::sync::mpsc::SyncSender<()>) -> Self {
        Self {
            unwind_tx: std::sync::Mutex::new(Some(unwind_tx)),
        }
    }
}

#[cfg(all(test, not(windows)))]
impl atm_core::boundary::sealed::Sealed for PanicDispatcherWithUnwindSignal {}

#[cfg(all(test, not(windows)))]
impl ApiRouter for PanicDispatcherWithUnwindSignal {
    fn route(
        &self,
        request: ApiRequest,
        _ingress: AuthenticatedIngress,
        _deadline: RequestDeadline,
    ) -> Result<ApiResponse, AtmError> {
        let unwind_signal = PanicUnwindSignal(
            self.unwind_tx
                .lock()
                .expect("panic dispatcher unwind signal lock")
                .take(),
        );
        let _keep_unwind_signal_until_panic_unwinds = unwind_signal;
        panic!(
            "intentional router panic for test: {:?}",
            request.into_inner()
        );
    }
}

pub(crate) fn sqlite_runtime_assembly_for_test(
    db_path: &std::path::Path,
) -> Result<RuntimeAssembly, AtmError> {
    atm_runtime_test_support::open_sqlite_boundary(db_path)
}

pub(crate) fn connect_daemon_local_ipc_until_ready(
    endpoint_path: &std::path::Path,
    ready_rx: std::sync::mpsc::Receiver<()>,
) -> LocalSocketStream {
    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("timed out waiting for daemon local ipc ready signal")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("daemon local ipc ready signal sender dropped before readiness")
        }
    }
    #[cfg(windows)]
    {
        let _endpoint_path = endpoint_path;
        let endpoint = atm_daemon_client::resolve_daemon_local_ipc_endpoint()
            .expect("windows local HTTP endpoint");
        match atm_daemon_client::try_connect(&endpoint)
            .expect("connect daemon local HTTP after ready signal")
        {
            atm_daemon_client::LocalDaemonConnection::TcpLoopback(stream) => stream,
        }
    }
    #[cfg(not(windows))]
    {
        let ipc_name = atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path)
            .expect("ipc name")
            .into_owned();
        let deadline = std::time::Instant::now() + TEST_LOCAL_IPC_CONNECT_DEADLINE;
        let mut attempts = 0usize;
        let mut retry_delay = TEST_LOCAL_IPC_CONNECT_RETRY_INITIAL_DELAY;
        let mut last_error = None;
        while std::time::Instant::now() < deadline {
            match connect_local_ipc_with_timeout(ipc_name.clone(), TEST_LOCAL_IPC_CONNECT_DEADLINE)
            {
                Ok(stream) => return stream,
                Err(error) => {
                    attempts += 1;
                    last_error = Some(error);
                    // The ready signal is the structural synchronization point; this retry only
                    // covers the residual OS socket publication race after readiness.
                    std::thread::sleep(retry_delay);
                    retry_delay = std::cmp::min(
                        retry_delay.saturating_mul(2),
                        TEST_LOCAL_IPC_CONNECT_RETRY_MAX_DELAY,
                    );
                }
            }
        }
        panic!(
            "connect daemon local ipc after ready signal failed after {attempts} attempts: {}",
            last_error
                .map(|error| error.to_string())
                .unwrap_or_else(|| "unknown connect error".to_string())
        )
    }
}

#[cfg(not(windows))]
pub(crate) fn connect_local_ipc_with_timeout(
    ipc_name: LocalSocketName<'static>,
    timeout: std::time::Duration,
) -> std::io::Result<LocalSocketStream> {
    let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("test-local-ipc-connect".to_string())
        .spawn(move || {
            let _ = result_tx.send(LocalSocketStream::connect(ipc_name));
        })
        .expect("spawn bounded local IPC connect helper");
    match result_rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "bounded local IPC connect timed out",
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "bounded local IPC connect worker disconnected",
        )),
    }
}

pub(crate) fn configure_test_local_ipc_timeouts(stream: &LocalSocketStream) {
    #[cfg(windows)]
    {
        apply_test_deadline(
            stream.set_write_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
            "set write timeout",
        );
        apply_test_deadline(
            stream.set_read_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
            "set read timeout",
        );
    }
    #[cfg(not(windows))]
    {
        apply_test_deadline(
            stream.set_send_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
            "set send timeout",
        );
        apply_test_deadline(
            stream.set_recv_timeout(Some(TEST_LOCAL_IPC_REQUEST_DEADLINE)),
            "set recv timeout",
        );
    }
}

pub(crate) fn write_test_local_ipc_request(
    stream: &mut LocalSocketStream,
    request: &RequestEnvelope,
) -> Result<(), AtmError> {
    #[cfg(windows)]
    {
        let endpoint = atm_daemon_client::resolve_daemon_local_ipc_endpoint()?;
        let record: atm_core::local_http::LocalHttpEndpointRecord =
            serde_json::from_slice(&std::fs::read(endpoint.as_ref()).map_err(|_source| {
                AtmError::daemon_unavailable("failed to read local HTTP endpoint record for test")
            })?)?;
        let capability = record.capability()?.to_base64url();
        atm_core::api::write_http_request_with_headers(
            stream,
            request,
            &[(
                atm_core::local_http::LOCAL_CAPABILITY_HEADER,
                capability.as_str(),
            )],
        )
    }
    #[cfg(not(windows))]
    {
        atm_core::api::write_http_request(stream, request)
    }
}

fn apply_test_deadline(result: std::io::Result<()>, context: &str) {
    if let Err(error) = result {
        #[cfg(windows)]
        {
            if error.kind() == std::io::ErrorKind::Unsupported {
                return;
            }
        }
        panic!("{context}: {error}");
    }
}
