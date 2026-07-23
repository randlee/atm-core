use atm_core::api::{ApiRequest, ApiResponse, ApiRouter, AuthenticatedIngress, RequestDeadline};
use atm_core::doctor::{DoctorEnvironmentVisibility, DoctorReport, DoctorStatus, DoctorSummary};
#[cfg(test)]
use atm_core::error::AtmError;
use atm_core::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_runtime::RuntimeAssembly;

use std::net::TcpStream;

use crate::lifecycle_control::LifecycleControlSourceAdapter;
const TEST_LOCAL_HTTP_REQUEST_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

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

pub(crate) fn sqlite_runtime_assembly_for_test(
    db_path: &std::path::Path,
) -> Result<RuntimeAssembly, AtmError> {
    atm_runtime_test_support::open_sqlite_boundary(db_path)
}

pub(crate) fn connect_daemon_local_http_until_ready(
    home_dir: &std::path::Path,
    ready_rx: std::sync::mpsc::Receiver<()>,
) -> (TcpStream, String) {
    match ready_rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("timed out waiting for daemon local HTTP ready signal")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("daemon local HTTP ready signal sender dropped before readiness")
        }
    }
    let record_path = atm_core::local_http::local_http_record_path(home_dir);
    let record: atm_core::local_http::LocalHttpEndpointRecord = serde_json::from_slice(
        &std::fs::read(record_path).expect("read local HTTP endpoint record"),
    )
    .expect("parse local HTTP endpoint record");
    let capability = record
        .capability()
        .expect("active local HTTP capability")
        .to_base64url();
    let endpoint = record
        .ipv4_loopback
        .expect("local HTTP IPv4 loopback endpoint");
    let stream =
        TcpStream::connect(endpoint).expect("connect daemon local HTTP after ready signal");
    (stream, capability)
}

pub(crate) fn configure_test_local_http_timeouts(stream: &TcpStream) {
    apply_test_deadline(
        stream.set_write_timeout(Some(TEST_LOCAL_HTTP_REQUEST_DEADLINE)),
        "set local HTTP write timeout",
    );
    apply_test_deadline(
        stream.set_read_timeout(Some(TEST_LOCAL_HTTP_REQUEST_DEADLINE)),
        "set local HTTP read timeout",
    );
}

pub(crate) fn write_test_local_http_request(
    stream: &mut TcpStream,
    request: &RequestEnvelope,
    capability: &str,
) -> Result<(), AtmError> {
    atm_core::api::write_http_request_with_headers(
        stream,
        request,
        &[(atm_core::local_http::LOCAL_CAPABILITY_HEADER, capability)],
    )
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
