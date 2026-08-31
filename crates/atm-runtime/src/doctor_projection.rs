//! Bounded Tokio projection for the daemon's control-plane doctor request.
//!
//! The HTTP adapter submits one typed request to this port.  Four independent
//! workers execute the retained core projection; callers wait only until their
//! request deadline and excess callers are rejected explicitly rather than
//! serialized behind the mailbox read bridge.

use std::sync::Arc;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::doctor::{
    DoctorExecutionContext, DoctorFinding, DoctorQuery, DoctorReport, DoctorSeverity,
    HerdrPresenceDoctor, RuntimeDoctorPorts, append_doctor_findings, run_doctor_with_runtime_ports,
};
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::RuntimeStatusSnapshot;
use atm_storage::AtmError;

use crate::{StateHandoffDiagnostics, SupervisorState};

/// Fixed bounded-control-plane settings for the doctor projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctorProjectionConfig {
    pub worker_count: usize,
    pub queue_depth: usize,
}

impl Default for DoctorProjectionConfig {
    fn default() -> Self {
        Self {
            worker_count: 4,
            queue_depth: 16,
        }
    }
}

/// In-process daemon details supplied by the HTTP composition layer.
#[derive(Debug, Clone, Default)]
pub struct DoctorProjectionContext {
    pub runtime_status: Option<RuntimeStatusSnapshot>,
    pub daemon_context: Option<DoctorExecutionContext>,
    pub handoff: Option<StateHandoffDiagnostics>,
}

#[async_trait::async_trait]
pub trait DoctorProjection: Send + Sync {
    async fn project(
        &self,
        query: DoctorQuery,
        context: DoctorProjectionContext,
        deadline: RequestDeadline,
    ) -> Result<DoctorReport, AtmError>;
}

/// Composition-owned bounded control lane.  Worker tasks are aborted when
/// the final projection clone drops, so test and daemon shutdown do not retain
/// a storage assembly through an orphaned background task.
#[derive(Clone)]
pub struct StorageDoctorProjection {
    sender: tokio::sync::mpsc::Sender<DoctorJob>,
    presence: Arc<dyn HerdrPresenceDoctor>,
    workers: Arc<DoctorWorkers>,
}

struct DoctorWorkers {
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for DoctorWorkers {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

struct DoctorJob {
    query: DoctorQuery,
    response: tokio::sync::oneshot::Sender<Result<DoctorReport, AtmError>>,
}

impl StorageDoctorProjection {
    pub fn start(
        config: DoctorProjectionConfig,
        runtime: LocalServiceRuntime,
        doctor_ports: RuntimeDoctorPorts,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
    ) -> Result<Self, AtmError> {
        if config.worker_count == 0 || config.queue_depth == 0 {
            return Err(AtmError::validation(
                "doctor projection worker count and queue depth must be non-zero",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            AtmError::daemon_unavailable("doctor projection must start inside the Tokio runtime")
        })?;
        let (sender, receiver) = tokio::sync::mpsc::channel(config.queue_depth);
        let presence = Arc::clone(&doctor_ports.herdr_presence);
        let receiver = Arc::new(tokio::sync::Mutex::new(receiver));
        let mut handles = Vec::with_capacity(config.worker_count);
        for _ in 0..config.worker_count {
            let receiver = Arc::clone(&receiver);
            let runtime = runtime.clone();
            let doctor_ports = doctor_ports.clone();
            let observability = Arc::clone(&observability);
            handles.push(tokio::spawn(async move {
                run_doctor_worker(receiver, runtime, doctor_ports, observability).await;
            }));
        }
        Ok(Self {
            sender,
            presence,
            workers: Arc::new(DoctorWorkers { handles }),
        })
    }
}

#[async_trait::async_trait]
impl DoctorProjection for StorageDoctorProjection {
    async fn project(
        &self,
        query: DoctorQuery,
        context: DoctorProjectionContext,
        deadline: RequestDeadline,
    ) -> Result<DoctorReport, AtmError> {
        let remaining = deadline.remaining().ok_or_else(|| {
            AtmError::daemon_unavailable(
                "doctor request deadline expired before control-lane admission",
            )
        })?;
        let (response, response_receiver) = tokio::sync::oneshot::channel();
        self.sender
            .try_send(DoctorJob { query, response })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    AtmError::daemon_connection_saturated("doctor control lane is saturated")
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    AtmError::daemon_unavailable("doctor control lane is unavailable")
                }
            })?;
        let mut report = tokio::time::timeout(remaining, response_receiver)
            .await
            .map_err(|_| AtmError::daemon_unavailable("doctor request deadline expired"))?
            .map_err(|_| AtmError::daemon_unavailable("doctor control lane stopped"))??;
        if let Some(roster) = report.member_roster.as_ref() {
            let remaining = deadline.remaining().ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "doctor request deadline expired before Herdr projection",
                )
            })?;
            match tokio::time::timeout(remaining, self.presence.probe(roster, deadline)).await {
                Ok(findings) => append_doctor_findings(&mut report, findings),
                Err(_) => append_doctor_findings(
                    &mut report,
                    vec![DoctorFinding {
                        severity: DoctorSeverity::Warning,
                        code: atm_storage::AtmErrorCode::DaemonUnavailable,
                        message: "Herdr presence projection exceeded the doctor request deadline"
                            .to_owned(),
                        remediation: Some(
                            "Inspect the Herdr service, then rerun `atm doctor`.".to_owned(),
                        ),
                    }],
                ),
            }
        }
        if let Some(runtime_status) = context.runtime_status {
            report.runtime_status = Some(runtime_status);
        }
        if let Some(handoff) = context.handoff
            && handoff.state != SupervisorState::Ready
        {
            append_doctor_findings(
                &mut report,
                vec![DoctorFinding {
                    severity: DoctorSeverity::Warning,
                    code: atm_storage::AtmErrorCode::DaemonUnavailable,
                    message: format!(
                        "mailbox read-state handoff is {:?}; buffered={}, restarts={}, rejected_full={}, rejected_unavailable={}, retry_deadline_exhaustions={}",
                        handoff.state,
                        handoff.buffered_depth,
                        handoff.restart_count,
                        handoff.rejected_buffer_full,
                        handoff.rejected_unavailable,
                        handoff.retry_deadline_exhaustions,
                    ),
                    remediation: Some(
                        "Inspect the mailbox writer lane and restart the daemon if the handoff does not recover."
                            .to_owned(),
                    ),
                }],
            );
        }
        report.daemon_context = context.daemon_context;
        Ok(report)
    }
}

impl StorageDoctorProjection {
    /// True only after the bounded worker set has fully stopped.
    #[must_use]
    pub fn workers_finished(&self) -> bool {
        self.workers
            .handles
            .iter()
            .all(tokio::task::JoinHandle::is_finished)
    }
}

async fn run_doctor_worker(
    receiver: Arc<tokio::sync::Mutex<tokio::sync::mpsc::Receiver<DoctorJob>>>,
    runtime: LocalServiceRuntime,
    doctor_ports: RuntimeDoctorPorts,
    observability: Arc<dyn ObservabilityPort + Send + Sync>,
) {
    loop {
        let Some(job) = receiver.lock().await.recv().await else {
            return;
        };
        let runtime = runtime.clone();
        let doctor_ports = doctor_ports.clone();
        let observability = Arc::clone(&observability);
        let result = tokio::task::spawn_blocking(move || {
            run_doctor_with_runtime_ports(
                job.query,
                observability.as_ref(),
                &runtime,
                &doctor_ports,
                None,
            )
        })
        .await
        .map_err(|error| AtmError::daemon_unavailable(format!("doctor worker failed: {error}")));
        let _ = job.response.send(result.and_then(|report| report));
    }
}
