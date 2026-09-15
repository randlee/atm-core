//! Periodic `$ATM_TEMP` TTL-sweep task composed against the replacement
//! Tokio/Axum runtime (ADR-055 decision (b)).
//!
//! This is the first periodic maintenance task composed against
//! `atm-daemon-bootstrap`/`atm-http-runtime` — not the legacy synchronous
//! daemon's maintenance worker, which CLAUDE.md rules off-limits for new
//! work. Its shutdown shape mirrors `WorkflowTelemetryRuntime::shutdown`
//! (`crates/atm-runtime/src/workflow_telemetry.rs`): send a cancellation
//! signal, give the worker its own bounded grace period to let an in-flight
//! sweep pass finish, and only abort — always followed by a join — if that
//! grace period expires. A raw `.abort()`-only shutdown does not guarantee
//! an in-progress sweep pass leaves the filesystem in a consistent state.
//!
//! The sweep pass itself is cooperatively cancellable (QM43-I7): it is
//! polled once per entry via a shared `AtomicBool`
//! (`atm_core::sweep_once_cancellable`), which `shutdown` sets before
//! waiting, so an in-flight pass over an unbounded tree stops promptly
//! instead of only being interruptible by an unconditional `.abort()` that
//! could leave a partially-processed directory in an inconsistent state.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use atm_core::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use atm_core::{SweepConfig, sweep_once_cancellable};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::DaemonLaunchIdentity;

/// Bounded grace period the sweeper is given to finish an in-flight sweep
/// pass after shutdown is requested, before its task is aborted (and, even
/// then, always joined).
pub const SWEEPER_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Owns the sweeper's periodic task. Constructed by [`AtmTempSweeperRuntime::start`]
/// and shut down by [`AtmTempSweeperRuntime::shutdown`] alongside the
/// daemon's other supervised workers.
pub struct AtmTempSweeperRuntime {
    shutdown: Mutex<Option<oneshot::Sender<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    cancelled: Arc<AtomicBool>,
}

impl AtmTempSweeperRuntime {
    /// Starts the periodic sweep task. The first pass runs immediately;
    /// subsequent passes run every `config.interval` until [`shutdown`] is
    /// called. Each pass reports through `observability` (when the daemon's
    /// launch identity carries a team/agent to attribute it to — mirroring
    /// `record_peer_wire_mode_selection`'s precedent) in addition to the
    /// existing structured `tracing` events, so a persistently failing
    /// sweeper is visible on the daemon's retained observability surface,
    /// not only by log-grepping.
    ///
    /// [`shutdown`]: Self::shutdown
    #[must_use]
    pub fn start(
        root: PathBuf,
        config: SweepConfig,
        observability: Arc<dyn ObservabilityPort + Send + Sync>,
        daemon_launch_identity: DaemonLaunchIdentity,
    ) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(config.interval);
            // M3: the default `Burst` catch-up behavior would fire the
            // sweeper back-to-back after any stall (a slow pass, a paused
            // process, system suspend) instead of resuming on its ordinary
            // cadence; a maintenance task has no reason to burst.
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let pass_root = root.clone();
                        let ttl = config.ttl;
                        let pass_cancelled = Arc::clone(&worker_cancelled);
                        let pass_observability = Arc::clone(&observability);
                        let pass_identity = daemon_launch_identity.clone();
                        // Run the sweep pass (synchronous filesystem I/O) on
                        // a blocking thread so it can never stall the async
                        // runtime. The pass itself polls `pass_cancelled`
                        // once per entry, so a shutdown signal that arrives
                        // mid-pass is observed within a bounded number of
                        // entries rather than only after the in-flight pass
                        // finishes naturally.
                        if let Err(error) = tokio::task::spawn_blocking(move || {
                            run_one_pass(
                                &pass_root,
                                ttl,
                                &pass_cancelled,
                                pass_observability.as_ref(),
                                &pass_identity,
                            );
                        })
                        .await
                        {
                            tracing::warn!(
                                subsystem = "atm_temp_sweeper",
                                action = "sweep",
                                outcome = "task_panicked",
                                %error,
                                "atm_temp sweep pass task panicked"
                            );
                        }
                    }
                    _ = &mut shutdown_rx => break,
                }
            }
        });
        Self {
            shutdown: Mutex::new(Some(shutdown_tx)),
            worker: Mutex::new(Some(worker)),
            cancelled,
        }
    }

    /// Signals shutdown, waits up to [`SWEEPER_SHUTDOWN_GRACE`] for an
    /// in-flight pass to finish, and joins the worker task — aborting only
    /// after the grace period expires, and always joining afterward so no
    /// detached task can outlive daemon shutdown.
    ///
    /// The cooperative cancellation flag is set immediately, before the
    /// grace-period wait begins: an in-flight blocking sweep pass polls it
    /// once per entry (`atm_core::sweep_once_cancellable`) and returns
    /// early, so shutdown does not need to wait for an unbounded directory
    /// walk to finish on its own (QM43-I7) — the grace period exists for
    /// the last in-flight chunk of work, not the whole remaining tree.
    pub async fn shutdown(&self) {
        if let Ok(mut sender) = self.shutdown.lock()
            && let Some(sender) = sender.take()
        {
            // A dropped-receiver error here just means the worker task has
            // already exited; nothing to react to.
            sender.send(()).ok();
        }
        self.cancelled.store(true, Ordering::Relaxed);
        let worker = self.worker.lock().ok().and_then(|mut worker| worker.take());
        let Some(mut worker) = worker else {
            return;
        };
        if tokio::time::timeout(SWEEPER_SHUTDOWN_GRACE, &mut worker)
            .await
            .is_err()
        {
            worker.abort();
            if let Err(error) = worker.await
                && !error.is_cancelled()
            {
                tracing::warn!(%error, "atm_temp sweeper abort join failed");
            }
        }
    }
}

impl Drop for AtmTempSweeperRuntime {
    fn drop(&mut self) {
        // The normal daemon path calls the async `shutdown` method and
        // drains within its own grace period. This is a fail-safe for an
        // early-return boot failure that drops the runtime without ever
        // calling `shutdown`: signal cancellation and abort rather than
        // leak a detached, possibly still-running task.
        self.cancelled.store(true, Ordering::Relaxed);
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            worker.abort();
        }
    }
}

fn run_one_pass(
    root: &Path,
    ttl: Duration,
    cancelled: &AtomicBool,
    observability: &dyn ObservabilityPort,
    daemon_launch_identity: &DaemonLaunchIdentity,
) {
    match sweep_once_cancellable(root, ttl, SystemTime::now(), cancelled) {
        Ok(report) => {
            tracing::info!(
                subsystem = "atm_temp_sweeper",
                action = "sweep",
                outcome = "completed",
                scanned = report.scanned,
                reclaimed_bytes = report.reclaimed_bytes,
                skipped = report.skipped,
                "atm_temp sweep pass completed"
            );
            record_sweep_pass_event(
                observability,
                daemon_launch_identity,
                "completed",
                Some(format!(
                    "scanned={} reclaimed_bytes={} skipped={}",
                    report.scanned, report.reclaimed_bytes, report.skipped
                )),
            );
        }
        Err(error) => {
            tracing::warn!(
                subsystem = "atm_temp_sweeper",
                action = "sweep",
                outcome = "failed",
                %error,
                "atm_temp sweep pass failed"
            );
            record_sweep_pass_event(
                observability,
                daemon_launch_identity,
                "failed",
                Some(error.to_string()),
            );
        }
    }
}

/// Reports one sweep pass through the daemon's retained observability
/// surface, mirroring `record_peer_wire_mode_selection`'s precedent
/// (`crates/atm-daemon-bootstrap/src/lib.rs`): `CommandEvent` requires a
/// non-optional team/agent/sender, so this only emits when the daemon's
/// launch identity supplies both, and otherwise relies on the `tracing`
/// event `run_one_pass` already recorded. This is what makes a
/// persistently failing sweeper visible on the daemon's health/observability
/// surface, not only by log-grepping (QM43-I4).
fn record_sweep_pass_event(
    observability: &dyn ObservabilityPort,
    daemon_launch_identity: &DaemonLaunchIdentity,
    outcome: &'static str,
    detail: Option<String>,
) {
    if let (Some(team), Some(identity)) = (
        daemon_launch_identity.team.clone(),
        daemon_launch_identity.identity.clone(),
    ) && let Err(error) = observability.emit(CommandEvent {
        command: "atm-daemon",
        action: action_name("atm_temp_sweep"),
        outcome: outcome_label(outcome),
        team,
        agent: identity.clone(),
        sender: identity,
        message_id: None,
        requires_ack: false,
        dry_run: false,
        task_id: None,
        error_code: None,
        error_message: detail,
    }) {
        tracing::warn!(%error, "failed to retain atm_temp sweep observability event");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        LogTailSession, NullObservability,
    };
    use atm_core::types::{AgentName, TeamName};
    use std::time::Duration;
    use tokio::sync::Notify;

    fn start_test_sweeper(root: PathBuf, config: SweepConfig) -> AtmTempSweeperRuntime {
        AtmTempSweeperRuntime::start(
            root,
            config,
            Arc::new(NullObservability),
            DaemonLaunchIdentity::default(),
        )
    }

    /// Test-only [`ObservabilityPort`] that notifies a [`Notify`] every time
    /// `run_one_pass` reports a completed (or failed) sweep pass through
    /// `record_sweep_pass_event`. This is the sweeper's own existing
    /// completion signal -- not a new production hook -- so
    /// `sweeper_reclaims_expired_entries_on_its_own_schedule` can await the
    /// real event a `spawn_blocking` pass fires when it finishes instead of
    /// polling with a fixed iteration cap, which is not a synchronization
    /// primitive and can under-wait on a loaded CI runner.
    #[derive(Default)]
    struct SweepPassCompletionObservability {
        pass_completed: Notify,
    }

    impl atm_core::boundary::sealed::Sealed for SweepPassCompletionObservability {}

    impl ObservabilityPort for SweepPassCompletionObservability {
        fn emit(&self, _event: CommandEvent) -> Result<(), atm_core::error::AtmError> {
            self.pass_completed.notify_one();
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, atm_core::error::AtmError> {
            Ok(AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, atm_core::error::AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, atm_core::error::AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: Some(AtmObservabilityHealthState::Unavailable),
                maintenance: None,
                diagnostic: None,
                jsonl: Default::default(),
                timeline: Default::default(),
                degraded: Vec::new(),
                detail: Some("sweep pass completion test observer".to_string()),
            })
        }
    }

    /// A `team`/`identity` pair is required for `record_sweep_pass_event` to
    /// call `observability.emit` at all (it silently no-ops otherwise, mirroring
    /// production's `daemon_launch_identity`-gated attribution), so tests that
    /// need the completion signal must supply one.
    fn test_daemon_launch_identity_with_team_and_agent() -> DaemonLaunchIdentity {
        DaemonLaunchIdentity {
            team: Some(TeamName::from_validated("test-team")),
            identity: Some(AgentName::from_validated("test-sweeper-agent")),
        }
    }

    // `start_paused` runs this test on tokio's virtual clock: no real
    // wall-clock wait is ever needed, and no `tokio::time::sleep` appears
    // anywhere in this module's tests (repo policy: fixed sleeps in test
    // code are rejected outright, not just discouraged).
    #[tokio::test(start_paused = true)]
    async fn sweeper_reclaims_expired_entries_on_its_own_schedule() {
        let dir = tempfile::tempdir().expect("tempdir");
        // A zero TTL makes any already-existing entry immediately expired
        // by both mtime and ctime, with no backdating needed: a real
        // inode's ctime cannot be backdated by any safe API (the kernel
        // always stamps it "now" on any metadata-changing syscall), so
        // `age_file`-style mtime backdating alone cannot deterministically
        // satisfy the dual mtime/ctime expiry check this test exercises
        // end-to-end through the real filesystem.
        let ttl = Duration::ZERO;
        let expired = dir.path().join("expired.bin");
        std::fs::write(&expired, b"x").expect("write");

        let observability = Arc::new(SweepPassCompletionObservability::default());
        let sweeper = AtmTempSweeperRuntime::start(
            dir.path().to_path_buf(),
            SweepConfig {
                interval: Duration::from_secs(3600),
                ttl,
            },
            Arc::clone(&observability) as Arc<dyn ObservabilityPort + Send + Sync>,
            test_daemon_launch_identity_with_team_and_agent(),
        );

        // `tokio::time::interval`'s first tick resolves immediately, so the
        // sweeper's first pass starts right away on a `spawn_blocking`
        // thread. Await the pass's own completion signal --
        // `record_sweep_pass_event`'s `observability.emit` call, which
        // `run_one_pass` fires unconditionally once the pass returns --
        // instead of polling with a fixed iteration cap: a cap is not a
        // synchronization primitive, and on a loaded CI runner the real
        // blocking thread can still be mid-pass when the cap is exhausted,
        // which is exactly the flake this replaces. `Notify::notified()` is
        // not a timer, so it is unaffected by this test's paused virtual
        // clock; a real, wall-clock-bounded `timeout` is intentionally not
        // used here since `start_paused`'s time auto-advance can race a
        // still-running `spawn_blocking` pass and fire the timeout before
        // the real thread finishes, reintroducing the exact kind of
        // under-wait flake this fix removes. A genuinely hung pass instead
        // fails loudly via the test harness's own overall timeout.
        observability.pass_completed.notified().await;
        assert!(!expired.exists(), "expired entry must be reclaimed");

        sweeper.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_joins_the_worker_without_a_bare_abort() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sweeper = start_test_sweeper(
            dir.path().to_path_buf(),
            SweepConfig {
                interval: Duration::from_secs(3600),
                ttl: Duration::from_secs(3600),
            },
        );
        // Shutdown must complete well within its own bounded grace period
        // even though the next tick is far in the future.
        tokio::time::timeout(SWEEPER_SHUTDOWN_GRACE, sweeper.shutdown())
            .await
            .expect("shutdown completes within its own grace period");
    }

    /// QM43-I7: a non-cancellable `spawn_blocking` sweep pass would let
    /// `.abort()` detach from an in-flight pass without actually stopping
    /// it, so `shutdown()` could "return" while the walk kept running (and
    /// deleting files) in the background. This exercises the exact
    /// `spawn_blocking` + shared-`AtomicBool` + `tokio::time::timeout`
    /// pattern `AtmTempSweeperRuntime` uses internally against a directory
    /// with many entries, and proves two things without any real sleep
    /// (deliberately: repo policy rejects fixed sleeps in test code
    /// outright, and a real sleep here would only be simulating slowness,
    /// not testing it) — a fake `EntryAgeSource` deterministically requests
    /// cancellation after a small, fixed number of entries have been
    /// visited, which is both fully deterministic (no timing dependency)
    /// and a faithful stand-in for "shutdown was requested mid-pass":
    ///
    /// 1. the pass's `JoinHandle` resolves well within
    ///    [`SWEEPER_SHUTDOWN_GRACE`] even though thousands of entries
    ///    remain unvisited;
    /// 2. the resulting [`atm_core::SweepReport`] proves it actually
    ///    stopped early (`scanned` is far below the entry count), not that
    ///    it happened to finish a full pass quickly.
    #[tokio::test(flavor = "multi_thread")]
    async fn shutdown_returns_within_the_bound_against_an_unbounded_walk() {
        use atm_core::{EntryAge, EntryAgeSource, sweep_once_with_age_source};
        use std::sync::atomic::AtomicUsize;

        const ENTRY_COUNT: usize = 5_000;
        const CANCEL_AFTER: usize = 10;

        let dir = tempfile::tempdir().expect("tempdir");
        for i in 0..ENTRY_COUNT {
            std::fs::write(dir.path().join(format!("f{i}.bin")), b"x").expect("write");
        }

        /// Deterministically requests cancellation after `threshold`
        /// entries have been visited, standing in for "shutdown was
        /// requested while the pass was partway through" without any
        /// timing dependency.
        struct CancelAfterN<'a> {
            visited: AtomicUsize,
            threshold: usize,
            cancelled: &'a AtomicBool,
        }

        impl EntryAgeSource for CancelAfterN<'_> {
            fn age_of(&self, _path: &Path, metadata: &std::fs::Metadata) -> EntryAge {
                if self.visited.fetch_add(1, Ordering::Relaxed) + 1 >= self.threshold {
                    self.cancelled.store(true, Ordering::Relaxed);
                }
                EntryAge::from_metadata(metadata)
            }
        }

        let cancelled = Arc::new(AtomicBool::new(false));
        let root = dir.path().to_path_buf();
        let pass_cancelled = Arc::clone(&cancelled);
        let pass = tokio::task::spawn_blocking(move || {
            let age_source = CancelAfterN {
                visited: AtomicUsize::new(0),
                threshold: CANCEL_AFTER,
                cancelled: pass_cancelled.as_ref(),
            };
            sweep_once_with_age_source(
                &root,
                Duration::from_secs(3600),
                SystemTime::now(),
                &age_source,
                &pass_cancelled,
            )
        });

        let report = tokio::time::timeout(SWEEPER_SHUTDOWN_GRACE, pass)
            .await
            .expect("cancelled pass must join within the shutdown grace period")
            .expect("blocking task must not panic")
            .expect("sweep_once_with_age_source must not error");

        assert!(
            (report.scanned as usize) < ENTRY_COUNT,
            "cancellation must stop the pass before every entry is visited, scanned={}",
            report.scanned
        );
    }
}
