//! Composes the replacement daemon's queue workers, Herdr doctor probes, and
//! `StorageAndNudgeRouter` assembly into a single handler.
//!
//! This is the seam between [`crate::assemble_daemon_runtime`]'s storage/peer
//! bootstrap and the runtime's request-handling boundary: [`compose_queue_workers`]
//! wires the received-hook selector to AQ3's recovery-sweep handle, the Herdr
//! doctor adapters translate spawn-breaker and presence-probe state into
//! doctor findings, and [`build_replacement_handler`] assembles all of it,
//! plus the peer connection pool and daemon execution context, into the
//! router the runtime serves.

use std::sync::Arc;
use std::time::Duration;

use atm_core::api::RequestDeadline;
use atm_core::doctor::{DoctorFinding, DoctorSeverity, HerdrPresenceDoctor};
use atm_core::error::AtmError;
use atm_core::observability::ObservabilityPort;
use atm_core::peer_wire::PeerWireMode;
use atm_core::team_admin::MembersList;
use atm_herdr::{
    BreakerPolicy, HerdrBreakerState, HerdrError, HerdrProcessAdapter, HerdrProcessInvoker,
    HerdrSpawnBreaker,
};
use atm_http_runtime::{
    HerdrQueueWakePump, PeerConnectionPool, PeerPoolConfig, PeerStreamAdapter, RuntimeHealth,
    StorageAndNudgeRouter, shared_direct_peer_client,
};
use atm_runtime::HandoffConfig;
use atm_runtime::RuntimeAssembly;

use crate::DaemonLaunchIdentity;
use crate::bare_cli_runtime::BareCliRuntime;
use crate::queue_drain;

/// The bootstrap-owned peer transport selection passed as one coherent unit
/// into replacement-daemon composition. Runtime code receives only the
/// opaque established-stream adapter and validated pool bounds.
pub(crate) struct SelectedPeerAdapterSelection {
    pub(crate) adapter: Option<Arc<dyn PeerStreamAdapter>>,
    pub(crate) pool_config: PeerPoolConfig,
}

pub(crate) struct ReplacementHandlerConfig<F> {
    pub(crate) observability: Arc<dyn ObservabilityPort + Send + Sync>,
    pub(crate) selector_factory: F,
    pub(crate) daemon_launch_identity: DaemonLaunchIdentity,
    pub(crate) peer_wire_mode: PeerWireMode,
    pub(crate) peer_adapter_selection: SelectedPeerAdapterSelection,
    pub(crate) runtime_health: RuntimeHealth,
    pub(crate) bare_cli: BareCliRuntime,
    pub(crate) herdr_process: Option<Arc<dyn HerdrProcessAdapter>>,
}

fn compose_queue_workers<F>(
    runtime: atm_core::LocalServiceRuntime,
    selector_factory: F,
    herdr_process: Arc<dyn HerdrProcessAdapter>,
    runtime_health: RuntimeHealth,
    bare_cli: BareCliRuntime,
) -> (
    Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    queue_drain::RecoverySweepHandle,
)
where
    F: FnOnce(
        atm_core::LocalServiceRuntime,
        Arc<dyn HerdrProcessAdapter>,
        RuntimeHealth,
        BareCliRuntime,
    ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
{
    let selector = selector_factory(
        runtime.clone(),
        herdr_process,
        runtime_health.clone(),
        bare_cli,
    );
    let recovery_sweep =
        queue_drain::spawn_recovery_sweep(runtime, selector.clone(), runtime_health);
    (selector, recovery_sweep)
}

struct HerdrBreakerDoctorAdapter {
    breaker: Arc<HerdrSpawnBreaker>,
}

impl atm_core::doctor::HerdrBreakerDoctor for HerdrBreakerDoctorAdapter {
    fn report(&self) -> atm_core::doctor::HerdrBreakerDoctorReport {
        let snapshot = self.breaker.snapshot();
        match snapshot.state {
            HerdrBreakerState::Closed => Default::default(),
            HerdrBreakerState::Open { retry_after } => atm_core::doctor::HerdrBreakerDoctorReport {
                state: atm_core::doctor::report::HerdrBreakerDoctorState::Open,
                retry_after_ms: Some(retry_after.as_millis() as u64),
                consecutive_failures: Some(snapshot.consecutive_failures),
            },
            HerdrBreakerState::HalfOpen => atm_core::doctor::HerdrBreakerDoctorReport {
                state: atm_core::doctor::report::HerdrBreakerDoctorState::Open,
                retry_after_ms: Some(0),
                consecutive_failures: Some(snapshot.consecutive_failures),
            },
        }
    }
}

pub(crate) struct HerdrPresenceDoctorAdapter {
    pub(crate) process: Arc<dyn HerdrProcessAdapter>,
}

impl HerdrPresenceDoctor for HerdrPresenceDoctorAdapter {
    fn probe<'a>(
        &'a self,
        roster: &'a MembersList,
        caller_deadline: RequestDeadline,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Vec<DoctorFinding>> + Send + 'a>> {
        Box::pin(probe_herdr_presence(
            Arc::clone(&self.process),
            roster,
            caller_deadline,
        ))
    }
}

async fn probe_herdr_presence(
    process: Arc<dyn HerdrProcessAdapter>,
    roster: &MembersList,
    caller_deadline: RequestDeadline,
) -> Vec<DoctorFinding> {
    let mut findings = Vec::new();
    let mut outage_reason = None;
    for member in roster.members.iter().filter(|member| {
        matches!(
            member.local_message_received_backend(),
            Some(atm_core::LocalMessageReceivedBackend::Herdr { .. })
        )
    }) {
        match probe_herdr_member(process.as_ref(), member, caller_deadline).await {
            Ok(()) => {}
            Err(error) if error.is_infrastructure() => {
                outage_reason.get_or_insert_with(|| format!("{error:?}"));
            }
            Err(error) => findings.push(herdr_presence_finding(error)),
        }
    }
    if let Some(reason) = outage_reason {
        findings.push(DoctorFinding {
            severity: DoctorSeverity::Info,
            code: atm_core::error_codes::AtmErrorCode::HerdrUnavailable,
            message: format!("Herdr presence probe skipped: {reason}"),
            remediation: None,
        });
    }
    findings
}

async fn probe_herdr_member(
    process: &dyn HerdrProcessAdapter,
    member: &atm_core::team_admin::MemberSummary,
    caller_deadline: RequestDeadline,
) -> Result<(), HerdrError> {
    let Some(atm_core::LocalMessageReceivedBackend::Herdr { session }) =
        member.local_message_received_backend()
    else {
        return Ok(());
    };
    let deadline = caller_deadline
        .remaining()
        .map(|remaining| RequestDeadline::after(remaining.min(Duration::from_secs(2))))
        .unwrap_or_else(|| RequestDeadline::after(Duration::ZERO));
    process
        .get(
            &member.name,
            session.as_ref(),
            deadline,
            BreakerPolicy::Bypass,
        )
        .await
        .map(|_| ())
}

pub(crate) fn herdr_presence_finding(error: HerdrError) -> DoctorFinding {
    let outcome = error.emission_outcome();
    if matches!(error, HerdrError::AgentNotFound) {
        let error = AtmError::new(
            atm_core::error_codes::AtmErrorCode::HerdrAgentNotVisible,
            "agent not visible in the member's configured Herdr session",
        );
        return DoctorFinding {
            severity: DoctorSeverity::Warning,
            code: error.code(),
            message: error.detail().to_owned(),
            remediation: Some(error.remediation().to_owned()),
        };
    }
    let error: AtmError = error.into();
    DoctorFinding {
        severity: DoctorSeverity::Warning,
        code: error.code(),
        message: format!(
            "Herdr presence probe outcome `{outcome}`: {}",
            error.detail()
        ),
        remediation: Some(error.remediation().to_owned()),
    }
}

pub(crate) fn build_replacement_handler(
    mut assembly: RuntimeAssembly,
    config: ReplacementHandlerConfig<
        impl FnOnce(
            atm_core::LocalServiceRuntime,
            Arc<dyn HerdrProcessAdapter>,
            RuntimeHealth,
            BareCliRuntime,
        ) -> Arc<dyn atm_core::boundary::MessageReceivedHookSelector>,
    >,
) -> Result<(Arc<StorageAndNudgeRouter>, queue_drain::RecoverySweepHandle), AtmError> {
    let ReplacementHandlerConfig {
        observability,
        selector_factory,
        daemon_launch_identity,
        peer_wire_mode,
        peer_adapter_selection,
        runtime_health,
        bare_cli,
        herdr_process,
    } = config;
    let herdr_process = resolve_herdr_process(&mut assembly, herdr_process);
    let queue_wake_process = Arc::clone(&herdr_process);
    let (selector, recovery_sweep) = compose_queue_workers(
        assembly.service_runtime.clone(),
        selector_factory,
        herdr_process,
        runtime_health.clone(),
        bare_cli.clone(),
    );
    let transition_sink = queue_drain::transition_sink(
        assembly.service_runtime.clone(),
        Arc::clone(&selector),
        runtime_health.clone(),
        recovery_sweep.transition_tracker(),
    );
    let queue_wake_pump = Arc::new(HerdrQueueWakePump::new(
        assembly.service_runtime.clone(),
        Arc::clone(&selector),
        runtime_health.clone(),
        queue_wake_process,
    ));
    let async_mailbox_runtime = assembly
        .async_mailbox_runtime
        .clone()
        .with_state_handoff(HandoffConfig::default())?;
    let handler = StorageAndNudgeRouter::new(
        assembly.service_runtime,
        observability,
        selector,
        atm_core::home::atm_home()?,
    )
    .with_async_mailbox_runtime(Arc::new(async_mailbox_runtime))
    .with_maintenance(queue_wake_pump)
    .with_runtime_health(runtime_health, assembly.doctor_ports)
    .with_member_state_transition_sink(transition_sink)
    .with_bare_cli_fifo(bare_cli.fifo(), bare_cli.queue_full_drops())
    .with_daemon_context(atm_core::doctor::DoctorExecutionContext {
        team: daemon_launch_identity.team.clone(),
        identity: daemon_launch_identity.identity.clone(),
        version: Some(atm_core::protocol::ReleaseVersion::current()),
        cli_schema_version: Some(atm_core::protocol::CLI_SCHEMA_VERSION),
        http_api_version: Some(atm_core::protocol::HttpApiVersion::current()),
        peer_wire_security: Some(peer_wire_mode.security().into()),
    })
    .with_shared_direct_peer_client(shared_direct_peer_client()?);
    let handler = match peer_adapter_selection.adapter {
        Some(adapter) => handler.with_peer_connection_pool(PeerConnectionPool::new(
            peer_adapter_selection.pool_config,
            adapter,
        )),
        None => handler,
    };
    Ok((Arc::new(handler), recovery_sweep))
}

fn resolve_herdr_process(
    assembly: &mut RuntimeAssembly,
    herdr_process: Option<Arc<dyn HerdrProcessAdapter>>,
) -> Arc<dyn HerdrProcessAdapter> {
    match herdr_process {
        Some(process) => process,
        None => {
            let herdr_breaker = Arc::new(HerdrSpawnBreaker::new());
            let process: Arc<dyn HerdrProcessAdapter> =
                Arc::new(HerdrProcessInvoker::new(Arc::clone(&herdr_breaker)));
            assembly.doctor_ports.herdr_breaker = Arc::new(HerdrBreakerDoctorAdapter {
                breaker: Arc::clone(&herdr_breaker),
            });
            assembly.doctor_ports.herdr_presence = Arc::new(HerdrPresenceDoctorAdapter {
                process: Arc::clone(&process),
            });
            process
        }
    }
}
