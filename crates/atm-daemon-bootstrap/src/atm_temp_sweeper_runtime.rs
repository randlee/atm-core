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

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use atm_core::{SweepConfig, sweep_once};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

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
}

impl AtmTempSweeperRuntime {
    /// Starts the periodic sweep task. The first pass runs immediately;
    /// subsequent passes run every `config.interval` until [`shutdown`] is
    /// called.
    ///
    /// [`shutdown`]: Self::shutdown
    #[must_use]
    pub fn start(root: PathBuf, config: SweepConfig) -> Self {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel::<()>();
        let worker = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(config.interval);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let pass_root = root.clone();
                        let ttl = config.ttl;
                        // Run the sweep pass (synchronous filesystem I/O) on
                        // a blocking thread so it can never stall the async
                        // runtime. Awaiting it here means a shutdown signal
                        // that arrives mid-pass is not observed until the
                        // in-flight pass finishes naturally.
                        if let Err(error) =
                            tokio::task::spawn_blocking(move || run_one_pass(&pass_root, ttl)).await
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
        }
    }

    /// Signals shutdown, waits up to [`SWEEPER_SHUTDOWN_GRACE`] for an
    /// in-flight pass to finish, and joins the worker task — aborting only
    /// after the grace period expires, and always joining afterward so no
    /// detached task can outlive daemon shutdown.
    pub async fn shutdown(&self) {
        if let Ok(mut sender) = self.shutdown.lock()
            && let Some(sender) = sender.take()
        {
            let _ = sender.send(());
        }
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
        // calling `shutdown`: abort rather than leak a detached task.
        if let Ok(mut worker) = self.worker.lock()
            && let Some(worker) = worker.take()
        {
            worker.abort();
        }
    }
}

fn run_one_pass(root: &Path, ttl: Duration) {
    match sweep_once(root, ttl, SystemTime::now()) {
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
        }
        Err(error) => {
            tracing::warn!(
                subsystem = "atm_temp_sweeper",
                action = "sweep",
                outcome = "failed",
                %error,
                "atm_temp sweep pass failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn age_file(path: &Path, age: Duration, now: SystemTime) {
        let modified = now.checked_sub(age).expect("age must not underflow");
        let file = std::fs::File::open(path).expect("open for mtime rewrite");
        file.set_modified(modified).expect("set mtime");
    }

    // `start_paused` runs this test on tokio's virtual clock: no real
    // wall-clock wait is ever needed, and no `tokio::time::sleep` appears
    // anywhere in this module's tests (repo policy: fixed sleeps in test
    // code are rejected outright, not just discouraged).
    #[tokio::test(start_paused = true)]
    async fn sweeper_reclaims_expired_entries_on_its_own_schedule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        let ttl = Duration::from_millis(1);
        let expired = dir.path().join("expired.bin");
        std::fs::write(&expired, b"x").expect("write");
        age_file(&expired, Duration::from_secs(3600), now);

        let sweeper = AtmTempSweeperRuntime::start(
            dir.path().to_path_buf(),
            SweepConfig {
                interval: Duration::from_secs(3600),
                ttl,
            },
        );

        // `tokio::time::interval`'s first tick resolves immediately, so the
        // sweeper's first pass starts right away; yield until its
        // `spawn_blocking` pass has had a chance to complete and notify this
        // task back, bounded by an iteration cap rather than a timer.
        for _ in 0..10_000 {
            if !expired.exists() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(!expired.exists(), "expired entry must be reclaimed");

        sweeper.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_joins_the_worker_without_a_bare_abort() {
        let dir = tempfile::tempdir().expect("tempdir");
        let sweeper = AtmTempSweeperRuntime::start(
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
}
