//! Test-only notification for one future run of a schedule generation.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use atm_core::{
    boundary::{CronGeneration, CronSchedule, CronScheduleId},
    error::AtmError,
};
use tokio::sync::oneshot;

type Key = (CronScheduleId, CronGeneration);

#[derive(Default)]
pub(crate) struct RunCompletionTracker {
    state: Mutex<TrackerState>,
}

#[derive(Default)]
struct TrackerState {
    closed: bool,
    schedules: HashMap<Key, ScheduleRuns>,
}

#[derive(Default)]
struct ScheduleRuns {
    started: u64,
    waiters: HashMap<u64, Vec<oneshot::Sender<Result<(), String>>>>,
}

impl RunCompletionTracker {
    pub(crate) fn register_next(&self, schedule: &CronSchedule) -> CronRunCompletionWaiter {
        let key = (schedule.id, schedule.generation);
        let (sender, receiver) = oneshot::channel();
        let mut state = self.state.lock().expect("cron completion tracker poisoned");
        if state.closed {
            let _ = sender.send(Err("cron worker stopped".into()));
        } else {
            let runs = state.schedules.entry(key).or_default();
            let target = runs.started.saturating_add(1);
            runs.waiters.entry(target).or_default().push(sender);
        }
        CronRunCompletionWaiter { receiver }
    }

    pub(crate) fn begin(self: &Arc<Self>, schedule: &CronSchedule) -> RunCompletionTicket {
        let key = (schedule.id, schedule.generation);
        let sequence = {
            let mut state = self.state.lock().expect("cron completion tracker poisoned");
            let runs = state.schedules.entry(key).or_default();
            runs.started = runs.started.saturating_add(1);
            runs.started
        };
        RunCompletionTicket {
            tracker: Arc::clone(self),
            key,
            sequence,
            finished: false,
        }
    }

    pub(crate) fn close(&self) {
        let mut state = self.state.lock().expect("cron completion tracker poisoned");
        state.closed = true;
        for runs in state.schedules.values_mut() {
            for (_, waiters) in runs.waiters.drain() {
                for waiter in waiters {
                    let _ = waiter.send(Err(
                        "cron worker stopped before the scheduled run completed".into(),
                    ));
                }
            }
        }
    }

    fn finish(&self, key: Key, sequence: u64, result: Result<(), String>) {
        let mut state = self.state.lock().expect("cron completion tracker poisoned");
        if let Some(waiters) = state
            .schedules
            .get_mut(&key)
            .and_then(|runs| runs.waiters.remove(&sequence))
        {
            for waiter in waiters {
                let _ = waiter.send(result.clone());
            }
        }
    }
}

pub struct CronRunCompletionWaiter {
    receiver: oneshot::Receiver<Result<(), String>>,
}

impl CronRunCompletionWaiter {
    pub async fn wait(self) -> Result<(), AtmError> {
        match self.receiver.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => Err(AtmError::daemon_unavailable(&reason)),
            Err(_) => Err(AtmError::daemon_unavailable(
                "cron run completion notification was dropped",
            )),
        }
    }
}

pub(crate) struct RunCompletionTicket {
    tracker: Arc<RunCompletionTracker>,
    key: Key,
    sequence: u64,
    finished: bool,
}

impl RunCompletionTicket {
    pub(crate) fn finish(&mut self, result: Result<(), String>) {
        self.tracker.finish(self.key, self.sequence, result);
        self.finished = true;
    }
}

impl Drop for RunCompletionTicket {
    fn drop(&mut self) {
        if !self.finished {
            self.tracker.finish(
                self.key,
                self.sequence,
                Err("cron run was cancelled before completion".into()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(id: u64, generation: u32) -> CronSchedule {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "generation": generation,
            "team": "test-team",
            "phase": "bk",
            "name": "barrier",
            "cron_expr": "0 * * * * *",
            "module_path": "/hook.py",
            "cwd": "/work",
            "args": [],
            "timeout_ms": 1000,
            "jitter_s": 0,
            "custom": {},
            "stop": {"kind": "hook"},
            "active": true,
            "created_by": "lumen",
            "created_at": "2026-10-24T00:00:00Z",
            "last_run_at": null,
            "last_outcome": null,
            "ended_at": null
        }))
        .expect("valid fixture schedule")
    }

    #[tokio::test]
    async fn waiter_ignores_prior_and_unrelated_runs_and_retains_fast_completion() {
        let tracker = Arc::new(RunCompletionTracker::default());
        let target = schedule(1, 1);

        let mut prior = tracker.begin(&target);
        prior.finish(Ok(()));
        let waiter = tracker.register_next(&target);
        let mut receiver = waiter.receiver;

        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        let mut other_schedule = tracker.begin(&schedule(2, 1));
        other_schedule.finish(Ok(()));
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        let mut other_generation = tracker.begin(&schedule(1, 2));
        other_generation.finish(Ok(()));
        assert!(matches!(
            receiver.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        let mut intended = tracker.begin(&target);
        intended.finish(Ok(()));
        assert_eq!(receiver.await.expect("completion sender retained"), Ok(()));
    }

    #[tokio::test]
    async fn closing_worker_releases_waiters_with_a_failure() {
        let tracker = RunCompletionTracker::default();
        let waiter = tracker.register_next(&schedule(1, 1));
        tracker.close();

        let error = waiter.wait().await.expect_err("worker closure is reported");
        assert_eq!(error.code().as_str(), "ATM_DAEMON_UNAVAILABLE");
    }
}
