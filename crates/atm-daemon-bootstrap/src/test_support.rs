//! Feature-gated deterministic daemon composition for CLI integration tests.
//!
//! This fixture installs the same typed cron execution path as the daemon. It
//! substitutes only test-owned storage, clock, jitter, and received-hook
//! transports; it does not expose the test-only cron constructor.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::{ApiRequest, AuthenticatedIngress, RequestDeadline};
use atm_core::boundary::{CronSchedule, PendingCronDelivery};
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
use atm_core::test_support::{TEST_RECIPIENT, TEST_SENDER, TEST_TEAM};
use atm_cron::{AtmCron, AtmCronTestAccess, CronWorkerHandle, ManualClock};
use atm_http_runtime::{
    CanonicalWriteHandler, PeerPoolConfig, RuntimeHealth, StorageAndNudgeRouter,
};
use atm_runtime::RuntimeAssembly;
use atm_runtime_test_support::open_isolated_sqlite_boundary;

use crate::cron_composition::{RuntimeCronComposition, compose_controlled};
use crate::replacement_handler::{
    ReplacementHandlerConfig, SelectedPeerAdapterSelection, build_replacement_handler,
};
use crate::{DaemonLaunchIdentity, queue_drain};

/// A deterministic, SQLite-backed composition for CLI integration tests.
///
/// Requests execute through the production replacement router, and cron is
/// installed through the production typed composition path. There is no
/// listener, fake scheduler, or environment-selected authority.
pub struct ControlledCronComposition {
    root: Arc<tempfile::TempDir>,
    runtime: LocalServiceRuntime,
    handler: Arc<StorageAndNudgeRouter>,
    cron: Arc<AtmCron>,
    clock: Arc<ManualClock>,
    cron_worker: Mutex<Option<CronWorkerHandle>>,
    herdr_process: Arc<atm_herdr::testing::FakeHerdrProcessAdapter>,
    recovery_sweep: queue_drain::RecoverySweepHandle,
}

impl ControlledCronComposition {
    /// Creates a fresh isolated SQLite composition with deterministic time.
    pub fn new() -> Result<Self, AtmError> {
        let root = tempfile::tempdir().map_err(|source| {
            AtmError::config("failed to create controlled cron fixture root").with_cause(source)
        })?;
        Self::from_root(Arc::new(root))
    }

    fn from_root(root: Arc<tempfile::TempDir>) -> Result<Self, AtmError> {
        let assembly = open_isolated_sqlite_boundary(root.path())?.for_daemon();
        Self::seed_roster(&assembly)?;
        let runtime = assembly.service_runtime.clone();
        let herdr_process = Arc::new(atm_herdr::testing::FakeHerdrProcessAdapter::default());
        let crate::replacement_handler::ReplacementHandler {
            handler,
            recovery_sweep,
            runner,
            cron_runner,
            dispatcher,
            local_host,
            lifecycle_nudge_emitter,
            ..
        } = build_replacement_handler(
            assembly,
            ReplacementHandlerConfig {
                observability: Arc::new(NullObservability),
                selector_factory:
                    |runtime, herdr, health, bare_cli: crate::bare_cli_runtime::BareCliRuntime| {
                        crate::received_hook_selector::active_received_hook_transports_with_health_and_fifo(
                        runtime,
                        herdr,
                        health,
                        bare_cli.fifo(),
                        bare_cli.queue_full_drops(),
                    )
                    },
                daemon_launch_identity: DaemonLaunchIdentity::default(),
                peer_wire_mode: atm_core::peer_wire::PeerWireMode::plaintext_test(),
                peer_adapter_selection: SelectedPeerAdapterSelection {
                    adapter: None,
                    pool_config: PeerPoolConfig::default(),
                },
                runtime_health: RuntimeHealth::default(),
                diagnostic_counters: None,
                bare_cli: Default::default(),
                herdr_config: Default::default(),
                herdr_process: Some(herdr_process.clone()),
                daemon_home: root.path().into(),
            },
        )?;
        let clock = Arc::new(ManualClock::new(
            "2026-10-24T00:00:00Z"
                .parse()
                .expect("fixed test timestamp is valid"),
        ));
        let cron = compose_controlled(
            RuntimeCronComposition {
                handler: handler.as_ref(),
                runtime: &runtime,
                runner,
                dispatcher,
                host: local_host,
                cron_runner,
                lifecycle_nudge_emitter,
            },
            Arc::clone(&clock),
        )?;
        Ok(Self {
            root,
            runtime,
            handler,
            cron,
            clock,
            cron_worker: Mutex::new(None),
            herdr_process,
            recovery_sweep,
        })
    }

    fn seed_roster(assembly: &RuntimeAssembly) -> Result<(), AtmError> {
        let team: atm_core::types::TeamName = TEST_TEAM.parse()?;
        assembly
            .service_runtime
            .shared_roster_store_arc()
            .save_roster(&atm_storage::RosterSnapshot {
                team_name: team.clone(),
                members: [TEST_SENDER, TEST_RECIPIENT]
                    .into_iter()
                    .map(|name| atm_core::boundary::RosterEntry {
                        team_name: team.clone(),
                        agent_name: name.parse().expect("fixed test agent name is valid"),
                        member_kind: atm_storage::RosterMemberKind::Permanent,
                        harness: atm_storage::RosterHarness::PythonGraft,
                        agent_type: Default::default(),
                        model: Default::default(),
                        recipient_pane_id: None,
                        metadata_json: if name == TEST_RECIPIENT {
                            serde_json::json!({
                                atm_core::delivery_channel::BACKEND_TYPE_METADATA_KEY: "herdr",
                                "alias": TEST_RECIPIENT
                            })
                            .as_object()
                            .expect("fixed test metadata is an object")
                            .clone()
                        } else {
                            Default::default()
                        },
                    })
                    .collect(),
                refreshed_at: None,
            })
    }

    /// Executes one decoded CLI request through the production router.
    pub async fn execute(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError> {
        CanonicalWriteHandler::dispatch(
            self.handler.as_ref(),
            ApiRequest::new(request),
            AuthenticatedIngress::Local,
            RequestDeadline::after(Duration::from_secs(60)),
        )
        .await
        .map(atm_core::api::ApiResponse::into_inner)
    }

    /// Advances the fixture's owned clock without timing or scheduler races.
    pub fn advance_clock(&self, duration: chrono::Duration) {
        self.clock.advance(duration);
    }

    /// Returns the isolated working directory supplied to real hook modules.
    pub fn root_path(&self) -> &Path {
        self.root.path()
    }

    /// Starts the production cron worker and waits until its manual clock is parked.
    pub async fn start_cron_worker(&self) -> Result<(), AtmError> {
        let worker = self.cron.start_worker().await?;
        {
            let mut slot = self
                .cron_worker
                .lock()
                .expect("controlled cron worker lock poisoned");
            debug_assert!(slot.is_none(), "cron worker can only start once");
            *slot = Some(worker);
        }
        self.clock.wait_parks_above(0).await;
        Ok(())
    }

    /// Advances the manual clock to a completed worker cycle without wall-clock sleeps.
    pub async fn advance_clock_settled(&self, duration: chrono::Duration) {
        self.clock.advance_settled(duration).await;
    }

    /// Advances the controlled clock and waits for this schedule generation's
    /// next run to finish dispatch and durable outcome recording.
    pub async fn advance_clock_and_wait_for_run(
        &self,
        schedule: &CronSchedule,
        duration: chrono::Duration,
    ) -> Result<(), AtmError> {
        self.advance_clock_and_wait_for_runs(&[schedule], duration)
            .await
    }

    /// Advances once after registering completions for every supplied schedule.
    ///
    /// Registering each waiter before moving the controlled clock makes a shared
    /// tick observable without a second advance that could trigger an unrelated
    /// recurrence.
    pub async fn advance_clock_and_wait_for_runs(
        &self,
        schedules: &[&CronSchedule],
        duration: chrono::Duration,
    ) -> Result<(), AtmError> {
        let completions = schedules
            .iter()
            .map(|schedule| self.cron.register_run_completion(schedule))
            .collect::<Vec<_>>();
        self.clock.advance_settled(duration).await;
        for completion in completions {
            completion.wait().await?;
        }
        Ok(())
    }

    /// Reads the durable cron state after a controlled run barrier completes.
    ///
    /// This test-support diagnostic deliberately observes the production
    /// schedule and transition stores rather than inferring delivery from
    /// worker completion alone. Pending intents retain the canonical payload
    /// and any reconciliation cause when delivery did not finish.
    pub fn persisted_cron_diagnostics(
        &self,
    ) -> Result<(Vec<CronSchedule>, Vec<PendingCronDelivery>), AtmError> {
        let schedules = self.runtime.cron_schedule_store()?.active_all()?;
        let pending = self.runtime.cron_transition_store()?.pending(128)?;
        Ok((schedules, pending))
    }

    /// Stops the production worker after all in-flight deterministic work settles.
    pub async fn shutdown_cron_worker(&self) {
        let worker = self
            .cron_worker
            .lock()
            .expect("controlled cron worker lock poisoned")
            .take();
        if let Some(worker) = worker {
            worker
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
                .await;
        }
    }

    /// Returns the direct Herdr calls made by the production lifecycle transport.
    pub fn herdr_calls(&self) -> Vec<atm_herdr::testing::FakeHerdrCall> {
        self.herdr_process.calls()
    }

    /// Whether the production lifecycle transport emitted a concrete Herdr prompt.
    #[must_use]
    pub fn has_herdr_prompt(&self, recipient: &str, expected_text: &str) -> bool {
        self.herdr_process.calls().iter().any(|call| {
            matches!(
                call,
                atm_herdr::testing::FakeHerdrCall::Prompt { agent, text, .. }
                    if agent == recipient && text == expected_text
            )
        })
    }

    /// Rebuilds the production composition over the same isolated SQLite data.
    pub async fn reopen(self) -> Result<Self, AtmError> {
        self.shutdown_cron_worker().await;
        let Self {
            root,
            runtime,
            handler,
            cron,
            clock,
            cron_worker: _,
            herdr_process: _,
            recovery_sweep,
        } = self;
        recovery_sweep
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(1))
            .await;
        drop((handler, cron, clock, runtime));
        Self::from_root(root)
    }
}

#[cfg(test)]
mod tests {
    use atm_core::hook::CronListQuery;
    use atm_core::protocol::{RequestEnvelope, ResponseEnvelope};
    use atm_core::test_support::{TEST_SENDER, TEST_TEAM};

    use super::ControlledCronComposition;

    #[tokio::test]
    async fn composition_executes_and_reopens_over_the_same_sqlite_state() {
        let composition = ControlledCronComposition::new().expect("compose fixture");
        let list = || {
            RequestEnvelope::CronList(CronListQuery {
                caller_identity: TEST_SENDER.parse().expect("fixed test sender"),
                caller_team: TEST_TEAM.parse().expect("fixed test team"),
                phase: None,
            })
        };

        assert!(matches!(
            composition.execute(list()).await.expect("execute list"),
            ResponseEnvelope::CronList(rows) if rows.is_empty()
        ));

        let reopened = composition.reopen().await.expect("reopen fixture");
        assert!(matches!(
            reopened.execute(list()).await.expect("execute reopened list"),
            ResponseEnvelope::CronList(rows) if rows.is_empty()
        ));
    }
}
