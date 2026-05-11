use atm_core::boundary::RequestDispatcher;
use atm_core::doctor::{DoctorEnvironmentVisibility, DoctorReport, DoctorStatus, DoctorSummary};
use atm_core::observability::{AtmObservabilityHealth, AtmObservabilityHealthState};
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};

#[cfg(unix)]
use interprocess::local_socket::Stream as LocalSocketStream;
#[cfg(unix)]
use interprocess::local_socket::traits::Stream as _;

use crate::lifecycle_control::LifecycleControlSourceAdapter;

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
        if let Err(error) = self.lifecycle.shutdown_worker_with_timeout() {
            tracing::warn!(
                %error,
                "failed to drain shared lifecycle worker during test reset"
            );
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct DoctorOnlyDispatcher;

impl atm_core::boundary::sealed::Sealed for DoctorOnlyDispatcher {}

impl RequestDispatcher for DoctorOnlyDispatcher {
    fn dispatch(
        &self,
        request: RequestEnvelope,
    ) -> Result<ResponseEnvelope, atm_core::error::AtmError> {
        match request {
            RequestEnvelope::Doctor(_) => Ok(ResponseEnvelope::Doctor(DoctorReport {
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
                member_roster: None,
                observability: AtmObservabilityHealth {
                    active_log_path: None,
                    logging_state: AtmObservabilityHealthState::Healthy,
                    query_state: Some(AtmObservabilityHealthState::Healthy),
                    detail: None,
                },
                runtime_status: None,
            })),
            other => panic!("unexpected request in DoctorOnlyDispatcher: {other:?}"),
        }
    }
}

#[cfg(unix)]
pub(crate) fn connect_daemon_local_ipc_until_ready(
    endpoint_path: &std::path::Path,
) -> LocalSocketStream {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match LocalSocketStream::connect(
            atm_core::protocol::daemon_local_ipc_name_from_path(endpoint_path).expect("ipc name"),
        ) {
            Ok(stream) => return stream,
            Err(error) if std::time::Instant::now() < deadline => {
                let _ = error;
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => panic!("connect daemon local ipc: {error}"),
        }
    }
}
