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
    DoctorTeamScope, HerdrEndpointDoctor, ReaderPoolDoctorReport, RuntimeDoctorPorts,
    append_doctor_findings, presence_findings_for_team, run_doctor_with_runtime_ports,
};
use atm_core::herdr_configured::herdr_is_configured;
use atm_core::observability::ObservabilityPort;
use atm_core::protocol::RuntimeStatusSnapshot;
use atm_storage::AtmError;

use crate::{StateHandoffDiagnostics, SupervisorState};

/// Fixed bounded-control-plane settings for the doctor projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctorProjectionConfig {
    pub worker_count: usize,
    pub queue_depth: usize,
    /// Effective capacities selected by the live storage assembly. The field
    /// remains absent for projections not attached to a running daemon.
    pub reader_lanes: Option<ReaderPoolDoctorReport>,
}

impl Default for DoctorProjectionConfig {
    fn default() -> Self {
        Self {
            worker_count: 4,
            queue_depth: 16,
            reader_lanes: None,
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
    endpoint_doctor: Arc<dyn HerdrEndpointDoctor>,
    reader_lanes: Option<ReaderPoolDoctorReport>,
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
            return Err(AtmError::validation_with_recovery(
                "doctor projection worker count and queue depth must be non-zero",
                "configure at least one doctor worker and one queue slot, then restart the runtime.",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            AtmError::daemon_unavailable_with_recovery(
                "doctor projection must start inside the Tokio runtime",
                "start the HTTP runtime under Tokio and retry the doctor request.",
            )
        })?;
        let (sender, receiver) = tokio::sync::mpsc::channel(config.queue_depth);
        let endpoint_doctor = Arc::clone(&doctor_ports.herdr_endpoint);
        let reader_lanes = config.reader_lanes;
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
            endpoint_doctor,
            reader_lanes,
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
            AtmError::daemon_unavailable_with_recovery(
                "doctor request deadline expired before control-lane admission",
                "increase the request deadline or retry when the daemon is ready.",
            )
        })?;
        let (response, response_receiver) = tokio::sync::oneshot::channel();
        self.sender
            .try_send(DoctorJob { query, response })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    AtmError::daemon_connection_saturated_with_recovery(
                        "doctor control lane is saturated",
                        "wait for an in-flight doctor request to finish, then retry.",
                    )
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => {
                    AtmError::daemon_unavailable_with_recovery(
                        "doctor control lane is unavailable",
                        "restart the HTTP runtime and retry the doctor request.",
                    )
                }
            })?;
        let mut report = tokio::time::timeout(remaining, response_receiver)
            .await
            .map_err(|_| {
                AtmError::daemon_unavailable_with_recovery(
                    "doctor request deadline expired",
                    "increase the request deadline or retry when the daemon is ready.",
                )
            })?
            .map_err(|_| {
                AtmError::daemon_unavailable_with_recovery(
                    "doctor control lane stopped",
                    "restart the HTTP runtime and retry the doctor request.",
                )
            })??;
        let rosters = report
            .member_roster
            .clone()
            .into_iter()
            .chain(report.team_rosters.clone())
            .collect::<Vec<_>>();
        if rosters.is_empty() {
            // Without a resolved team there is no Herdr-backed member to
            // inspect. This is a known, unconfigured state rather than a
            // guessed endpoint result.
            report.herdr.configured = Some(false);
        } else {
            let per_team_budget = deadline
                .remaining()
                .map(|remaining| remaining / rosters.len() as u32);
            let scope = report.resolved_team_scope.clone();
            for roster in rosters {
                if deadline.expired() {
                    append_deadline_finding(
                        &mut report,
                        "doctor request deadline expired before the next Herdr team projection",
                    );
                    break;
                }
                self.append_herdr_report(&mut report, &roster, &scope, deadline, per_team_budget)
                    .await;
            }
        }
        append_context_findings(&mut report, context);
        report.reader_lanes = self.reader_lanes;
        Ok(report)
    }
}

impl StorageDoctorProjection {
    async fn append_herdr_report(
        &self,
        report: &mut DoctorReport,
        roster: &atm_core::team_admin::MembersList,
        scope: &DoctorTeamScope,
        caller_deadline: RequestDeadline,
        budget: Option<std::time::Duration>,
    ) {
        let configured = herdr_is_configured(roster);
        report.herdr.configured = Some(report.herdr.configured.unwrap_or(false) || configured);
        let Some(remaining) = caller_deadline.remaining() else {
            append_deadline_finding(
                report,
                "doctor request deadline expired before Herdr projection",
            );
            return;
        };
        let timeout_budget = budget.map_or(remaining, |budget| budget.min(remaining));
        match tokio::time::timeout(
            timeout_budget,
            self.endpoint_doctor.observe(roster, caller_deadline),
        )
        .await
        {
            Ok(observations) => {
                let findings = if scope.is_all_teams() {
                    presence_findings_for_team(&observations, &roster.team)
                } else {
                    atm_core::doctor::presence_findings(&observations)
                };
                report
                    .herdr
                    .endpoints
                    .extend(observations.into_iter().map(Into::into));
                append_doctor_findings(report, findings);
            }
            Err(_) => {
                let finding = herdr_deadline_finding();
                report.herdr.error = Some(finding.clone());
                append_doctor_findings(report, vec![finding]);
            }
        }
    }
}

fn herdr_deadline_finding() -> DoctorFinding {
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: atm_storage::AtmErrorCode::DaemonUnavailable,
        message: "Herdr presence projection exceeded its per-team doctor budget".to_owned(),
        remediation: Some("Inspect the Herdr service, then rerun `atm doctor`.".to_owned()),
    }
}

fn append_deadline_finding(report: &mut DoctorReport, message: &str) {
    let finding = DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: atm_storage::AtmErrorCode::DaemonUnavailable,
        message: message.to_owned(),
        remediation: Some(
            "Increase the doctor request deadline, then rerun `atm doctor`.".to_owned(),
        ),
    };
    report.herdr.error = Some(finding.clone());
    append_doctor_findings(report, vec![finding]);
}

fn append_context_findings(report: &mut DoctorReport, context: DoctorProjectionContext) {
    if let Some(runtime_status) = context.runtime_status {
        report.runtime_status = Some(runtime_status.clone());
        append_doctor_findings(
            report,
            atm_core::doctor::runtime_condition_findings(&runtime_status),
        );
    }
    if let Some(handoff) = context.handoff
        && handoff.state != SupervisorState::Ready
    {
        append_doctor_findings(
            report,
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
        .map_err(|error| {
            AtmError::daemon_unavailable_with_recovery(
                format!("doctor worker failed: {error}"),
                "restart the HTTP runtime and retry the doctor request.",
            )
        });
        let _ = job.response.send(result.and_then(|report| report));
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use atm_core::api::RequestDeadline;
    use atm_core::doctor::{DoctorQuery, HerdrEndpointDoctor, RuntimeDoctorPorts};
    use atm_core::observability::NullObservability;
    use atm_core::team_admin::MembersList;
    use atm_core::types::{AgentName, TeamName};
    use atm_runtime_test_support::open_isolated_sqlite_boundary;
    use atm_storage::{RosterHarness, RosterMember, RosterMemberKind, RosterSnapshot};

    use super::{
        DoctorProjection, DoctorProjectionConfig, DoctorProjectionContext, StorageDoctorProjection,
    };

    #[derive(Clone)]
    struct CountingEndpoint {
        calls: Arc<AtomicUsize>,
    }

    impl atm_core::boundary::sealed::Sealed for CountingEndpoint {}

    impl HerdrEndpointDoctor for CountingEndpoint {
        fn observe<'a>(
            &'a self,
            _roster: &'a MembersList,
            _caller_deadline: RequestDeadline,
        ) -> Pin<
            Box<dyn Future<Output = Vec<atm_core::doctor::HerdrEndpointObservation>> + Send + 'a>,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Vec::new() })
        }
    }

    fn roster(team: &str) -> RosterSnapshot {
        let team = TeamName::from_validated(team);
        RosterSnapshot {
            team_name: team.clone(),
            members: vec![RosterMember {
                team_name: team,
                agent_name: AgentName::from_validated("member"),
                member_kind: RosterMemberKind::Permanent,
                harness: RosterHarness::ClaudeCode,
                agent_type: atm_storage::AgentType::Worker,
                model: Default::default(),
                recipient_pane_id: None,
                metadata_json: serde_json::Map::new(),
            }],
            refreshed_at: None,
        }
    }

    #[tokio::test]
    async fn all_team_projection_aggregates_each_roster_through_the_async_loop() {
        let root = std::env::temp_dir().join(format!(
            "atm-runtime-doctor-projection-{}",
            atm_storage::AtmMessageId::new()
        ));
        std::fs::create_dir_all(&root).expect("temporary runtime root");
        let assembly = open_isolated_sqlite_boundary(&root).expect("runtime assembly");
        let roster_store = assembly.shared_roster_store_arc();
        roster_store
            .save_roster(&roster("team-a"))
            .expect("team-a roster");
        roster_store
            .save_roster(&roster("team-b"))
            .expect("team-b roster");

        let calls = Arc::new(AtomicUsize::new(0));
        let endpoint = Arc::new(CountingEndpoint {
            calls: Arc::clone(&calls),
        });
        let ports = RuntimeDoctorPorts {
            config_doctor: Arc::clone(&assembly.doctor_ports.config_doctor),
            mail_store_doctor: Arc::clone(&assembly.doctor_ports.mail_store_doctor),
            roster_store_doctor: Arc::clone(&assembly.doctor_ports.roster_store_doctor),
            herdr_breaker: Arc::clone(&assembly.doctor_ports.herdr_breaker),
            herdr_endpoint: endpoint,
        };
        let projection = StorageDoctorProjection::start(
            DoctorProjectionConfig::default(),
            assembly.service_runtime.clone(),
            ports,
            Arc::new(NullObservability),
        )
        .expect("doctor projection");

        let report = projection
            .project(
                DoctorQuery {
                    home_dir: root.clone(),
                    current_dir: root.clone(),
                    all_teams: true,
                    ..DoctorQuery::default()
                },
                DoctorProjectionContext::default(),
                RequestDeadline::after(std::time::Duration::from_secs(5)),
            )
            .await
            .expect("all-team projection");

        assert_eq!(report.team_rosters.len(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        std::fs::remove_dir_all(root).expect("remove temporary runtime root");
    }
}
