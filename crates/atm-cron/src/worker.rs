use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use atm_core::{
    boundary::{CronSchedule, CronScheduleId, StopCondition},
    error::AtmError,
};
use chrono::{DateTime, Duration, Utc};
use tokio::{
    sync::{oneshot, watch},
    task::{JoinError, JoinHandle},
};

use crate::{
    AtmCron, expr,
    scheduler::store_call,
    tick::{self, CronRunOutcome},
};

struct Slot {
    schedule: CronSchedule,
    next: Option<DateTime<Utc>>,
    in_flight: Option<JoinHandle<CronRunOutcome>>,
}

struct Running(watch::Sender<usize>);

impl Running {
    fn new(sender: watch::Sender<usize>) -> Self {
        sender.send_modify(|count| *count += 1);
        Self(sender)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}

struct TrackedRun<F> {
    future: Pin<Box<F>>,
    _guard: Running,
}

impl<F: Future> Future for TrackedRun<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, context: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.get_mut().future.as_mut().poll(context)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(handle) = &self.in_flight {
            handle.abort();
        }
    }
}

pub(crate) struct SlotView<'a> {
    pub schedule: &'a CronSchedule,
    pub next: Option<DateTime<Utc>>,
    pub in_flight: bool,
}

#[derive(Default, Debug)]
pub(crate) struct Step {
    pub run: Vec<CronScheduleId>,
    pub skip_in_flight: Vec<CronScheduleId>,
    pub stop_until: Vec<CronScheduleId>,
    pub invalid_next: Vec<CronScheduleId>,
    pub wake: Option<DateTime<Utc>>,
}

fn next_time(
    schedule: &CronSchedule,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AtmError> {
    let jitter = Duration::seconds(i64::from(schedule.jitter_s));
    let Some(base) = now.checked_sub_signed(jitter) else {
        return Ok(None);
    };
    let next = expr::parse_syntax(&schedule.cron_expr)?
        .after(&base)
        .next()
        .and_then(|next| next.checked_add_signed(jitter));
    Ok(next)
}

pub(crate) fn plan_step(slots: &[SlotView<'_>], now: DateTime<Utc>) -> Step {
    let mut step = Step::default();
    for slot in slots {
        if tick::stop_due(&slot.schedule.stop, now) {
            step.stop_until.push(slot.schedule.id);
            continue;
        }
        let due = slot.next.is_some_and(|next| next <= now);
        if due {
            if slot.in_flight {
                step.skip_in_flight.push(slot.schedule.id);
            } else {
                step.run.push(slot.schedule.id);
            }
        }
        let next = if due {
            match next_time(slot.schedule, now) {
                Ok(next) => next,
                Err(_) => {
                    step.invalid_next.push(slot.schedule.id);
                    None
                }
            }
        } else {
            slot.next
        };
        let wake = match slot.schedule.stop {
            StopCondition::Until { at } => {
                Some(next.map_or(at.into_inner(), |next| next.min(at.into_inner())))
            }
            StopCondition::Hook => next,
        };
        if let Some(wake) = wake {
            step.wake = Some(step.wake.map_or(wake, |old| old.min(wake)));
        }
    }
    step
}

async fn stop_exhausted(cron: &AtmCron, schedule: &CronSchedule) {
    let outcome = CronRunOutcome::Stopped("no future occurrence".into());
    tick::record(cron, schedule, &outcome).await;
    tick::log(schedule, "tick", &outcome, 0);
}

fn log_next_time_error(schedule: &CronSchedule, error: &AtmError) {
    tracing::error!(
        subsystem = "atm_cron",
        action = "schedule_next_failed",
        schedule = %schedule.id,
        code = %error.code(),
        %error,
        "cron schedule next time failed: {error}"
    );
}

fn unchanged(left: &CronSchedule, right: &CronSchedule) -> bool {
    left.id == right.id
        && left.generation == right.generation
        && left.created_at == right.created_at
        && left.cron_expr == right.cron_expr
        && left.jitter_s == right.jitter_s
        && left.stop == right.stop
        && left.module_path == right.module_path
        && left.cwd == right.cwd
        && left.args == right.args
        && left.timeout_ms == right.timeout_ms
        && left.custom == right.custom
}

async fn merge(cron: &AtmCron, slots: &mut Vec<Slot>, rows: Vec<CronSchedule>) {
    let mut previous = std::mem::take(slots);
    for schedule in rows.into_iter().filter(|row| row.active) {
        if let Some(index) = previous
            .iter()
            .position(|slot| unchanged(&slot.schedule, &schedule))
        {
            slots.push(previous.remove(index));
        } else {
            match next_time(&schedule, cron.deps.clock.now()) {
                Ok(Some(next)) => slots.push(Slot {
                    schedule,
                    next: Some(next),
                    in_flight: None,
                }),
                Ok(None) => stop_exhausted(cron, &schedule).await,
                Err(error) => log_next_time_error(&schedule, &error),
            }
        }
    }
    for mut removed in previous {
        if let Some(handle) = removed.in_flight.take() {
            handle.abort();
            if let Err(error) = handle.await {
                tracing::info!(subsystem = "atm_cron", action = "cancelled", schedule = %removed.schedule.id,
                    %error, "cron run cancelled on reload: {error}");
            }
        }
    }
}

async fn finished(slots: &mut [Slot]) -> (usize, Result<CronRunOutcome, JoinError>) {
    poll_fn(|context| {
        for (index, slot) in slots.iter_mut().enumerate() {
            if let Some(handle) = &mut slot.in_flight
                && let Poll::Ready(result) = Pin::new(handle).poll(context)
            {
                return Poll::Ready((index, result));
            }
        }
        Poll::Pending
    })
    .await
}

async fn complete(cron: &AtmCron, slot: &Slot, result: Result<CronRunOutcome, JoinError>) -> bool {
    match result {
        Ok(CronRunOutcome::Stopped(_)) => true,
        Ok(_) => false,
        Err(error) if error.is_panic() => {
            let payload = error.into_panic();
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic");
            let outcome = CronRunOutcome::Exit(format!("hook runner panicked: {message}"));
            tick::log(&slot.schedule, "tick", &outcome, 0);
            tick::record(cron, &slot.schedule, &outcome).await;
            false
        }
        Err(error) => {
            tracing::info!(subsystem = "atm_cron", action = "cancelled", schedule = %slot.schedule.id,
                %error, "cron run cancelled: {error}");
            false
        }
    }
}

fn step_for(slots: &[Slot], now: DateTime<Utc>) -> Step {
    let views: Vec<_> = slots
        .iter()
        .map(|slot| SlotView {
            schedule: &slot.schedule,
            next: slot.next,
            in_flight: slot.in_flight.is_some(),
        })
        .collect();
    plan_step(&views, now)
}

/// Backoff for a failed schedule reload: 1s, 2s, 4s, ... doubling per failed
/// attempt and capped at one minute, so an outage is retried long before the
/// next occurrence of a schedule.
const RELOAD_RETRY_MAX_SECS: i64 = 60;

#[derive(Default)]
struct ReloadRetry {
    failures: u32,
    due: Option<DateTime<Utc>>,
}

impl ReloadRetry {
    fn delay(failures: u32) -> Duration {
        let secs = 1i64
            .checked_shl(failures)
            .unwrap_or(RELOAD_RETRY_MAX_SECS)
            .min(RELOAD_RETRY_MAX_SECS);
        Duration::seconds(secs)
    }

    fn failed(&mut self, now: DateTime<Utc>) {
        self.due = Some(now + Self::delay(self.failures));
        self.failures = self.failures.saturating_add(1);
    }

    fn succeeded(&mut self) {
        *self = Self::default();
    }

    fn is_due(&self, now: DateTime<Utc>) -> bool {
        self.due.is_some_and(|due| due <= now)
    }
}

async fn reload_slots(cron: &AtmCron, slots: &mut Vec<Slot>, retry: &mut ReloadRetry) {
    match store_call(cron.deps.store.clone(), |store| store.active_all()).await {
        Ok(rows) => {
            merge(cron, slots, rows).await;
            retry.succeeded();
        }
        Err(error) => {
            tracing::error!(subsystem = "atm_cron", action = "reload_failed", %error, "cron reload failed: {error}");
            retry.failed(cron.deps.clock.now());
        }
    }
}

async fn apply_step(cron: &Arc<AtmCron>, slots: &mut Vec<Slot>, running: &watch::Sender<usize>) {
    let now = cron.deps.clock.now();
    let step = step_for(slots, now);
    for index in (0..slots.len()).rev() {
        if step.invalid_next.contains(&slots[index].schedule.id) {
            let slot = slots.remove(index);
            if let Err(error) = next_time(&slot.schedule, now) {
                log_next_time_error(&slot.schedule, &error);
            }
        }
    }
    for id in &step.stop_until {
        if let Some(index) = slots.iter().position(|slot| slot.schedule.id == *id) {
            let mut slot = slots.remove(index);
            if let Some(handle) = slot.in_flight.take() {
                handle.abort();
                if let Err(error) = handle.await {
                    tracing::info!(subsystem = "atm_cron", action = "cancelled", schedule = %slot.schedule.id,
                        %error, "cron run cancelled at until: {error}");
                }
            }
            let outcome = CronRunOutcome::Stopped("until reached".into());
            tick::record(cron, &slot.schedule, &outcome).await;
            tick::log(&slot.schedule, "tick", &outcome, 0);
        }
    }
    let mut exhausted = Vec::new();
    for (index, slot) in slots.iter_mut().enumerate() {
        if step.skip_in_flight.contains(&slot.schedule.id) {
            tick::log(&slot.schedule, "tick", &CronRunOutcome::SkippedInFlight, 0);
            match next_time(&slot.schedule, now) {
                Ok(Some(next)) => slot.next = Some(next),
                Ok(None) => exhausted.push(index),
                Err(error) => log_next_time_error(&slot.schedule, &error),
            }
        } else if step.run.contains(&slot.schedule.id) {
            let owner = cron.clone();
            let schedule = slot.schedule.clone();
            let guard = Running::new(running.clone());
            slot.in_flight = Some(tokio::spawn(TrackedRun {
                future: Box::pin(async move { tick::run_one(&owner, &schedule).await }),
                _guard: guard,
            }));
            match next_time(&slot.schedule, now) {
                Ok(Some(next)) => slot.next = Some(next),
                Ok(None) => slot.next = None,
                Err(error) => log_next_time_error(&slot.schedule, &error),
            }
        }
    }
    for index in exhausted.into_iter().rev() {
        let slot = slots.remove(index);
        stop_exhausted(cron, &slot.schedule).await;
    }
}

pub(crate) async fn run(
    cron: Arc<AtmCron>,
    rows: Vec<CronSchedule>,
    mut reload: watch::Receiver<u64>,
    mut shutdown: oneshot::Receiver<()>,
    running: watch::Sender<usize>,
) {
    let mut slots = Vec::new();
    merge(&cron, &mut slots, rows).await;
    let mut reload_retry = ReloadRetry::default();
    loop {
        let now = cron.deps.clock.now();
        let step = step_for(&slots, now);
        let due =
            !step.run.is_empty() || !step.stop_until.is_empty() || !step.skip_in_flight.is_empty();
        let wake = if due {
            now
        } else {
            step.wake.unwrap_or(DateTime::<Utc>::MAX_UTC)
        };
        let wake = reload_retry.due.map_or(wake, |retry| wake.min(retry));
        tokio::select! {
            biased;
            result = reload.changed() => {
                if result.is_err() { break; }
                reload_slots(&cron, &mut slots, &mut reload_retry).await;
            }
            () = cron.deps.clock.sleep_until(wake) => {
                if reload_retry.is_due(cron.deps.clock.now()) {
                    reload_slots(&cron, &mut slots, &mut reload_retry).await;
                }
                apply_step(&cron, &mut slots, &running).await;
            }
            (index, result) = finished(&mut slots) => {
                slots[index].in_flight.take();
                if complete(&cron, &slots[index], result).await { slots.remove(index); }
                else {
                    match next_time(&slots[index].schedule, cron.deps.clock.now()) {
                        Ok(Some(next)) => slots[index].next = Some(next),
                        Ok(None) => {
                            let slot = slots.remove(index);
                            stop_exhausted(&cron, &slot.schedule).await;
                        }
                        Err(error) => {
                            log_next_time_error(&slots[index].schedule, &error);
                            slots.remove(index);
                        }
                    }
                }
            }
            _ = &mut shutdown => break,
        }
    }
    for mut slot in slots {
        if let Some(handle) = &mut slot.in_flight {
            let result = handle.await;
            slot.in_flight.take();
            complete(&cron, &slot, result).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atm_core::test_support::TEST_SENDER;

    #[test]
    fn reload_retry_doubles_per_failure_then_caps_at_one_minute() {
        let now = DateTime::<Utc>::UNIX_EPOCH;
        let mut retry = ReloadRetry::default();
        let mut delays = Vec::new();
        for _ in 0..9 {
            retry.failed(now);
            delays.push((retry.due.expect("failure sets a due time") - now).num_seconds());
        }
        assert_eq!(delays, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
        assert!(!retry.is_due(now));
        assert!(retry.is_due(now + Duration::seconds(60)));
    }

    #[test]
    fn reload_retry_success_clears_the_backoff() {
        let now = DateTime::<Utc>::UNIX_EPOCH;
        let mut retry = ReloadRetry::default();
        retry.failed(now);
        retry.failed(now);
        retry.succeeded();
        assert!(retry.due.is_none());
        retry.failed(now);
        assert_eq!(retry.due, Some(now + Duration::seconds(1)));
    }

    fn row() -> CronSchedule {
        serde_json::from_value(serde_json::json!({
            "id":1,"generation":1,"team":"test-team","phase":"bk","name":"test",
            "cron_expr":"@hourly","module_path":"/module.py","cwd":"/work","args":[],
            "timeout_ms":1000,"jitter_s":600,"custom":{},"stop":{"kind":"hook"},
            "active":true,"created_by":TEST_SENDER,"created_at":"2026-10-19T00:00:00Z",
            "last_run_at":null,"last_outcome":null,"ended_at":null
        }))
        .unwrap()
    }

    #[test]
    fn next_time_preserves_invalid_expression_error() {
        let mut row = row();
        row.cron_expr = "not a cron expression".into();

        let error = next_time(&row, row.created_at.into_inner()).unwrap_err();

        assert_eq!(error.code().as_str(), "ATM_CRON_EXPR_INVALID");
    }

    #[test]
    fn plan_step_fires_at_occurrence_plus_jitter() {
        let mut row = row();
        let next = "2026-10-19T10:10:00Z".parse().unwrap();
        let now = "2026-10-19T10:00:00Z".parse().unwrap();
        let view = SlotView {
            schedule: &row,
            next: Some(next),
            in_flight: false,
        };
        let step = plan_step(&[view], now);
        assert!(step.run.is_empty());
        assert_eq!(step.wake, Some(next));
        assert_eq!(
            plan_step(
                &[SlotView {
                    schedule: &row,
                    next: Some(next),
                    in_flight: false
                }],
                next
            )
            .run,
            vec![row.id]
        );
        let at: DateTime<Utc> = "2026-10-19T10:05:00Z".parse().unwrap();
        row.stop = StopCondition::Until { at: at.into() };
        let step = plan_step(
            &[SlotView {
                schedule: &row,
                next: Some(next),
                in_flight: false,
            }],
            now,
        );
        assert_eq!(step.wake, Some(at));
        assert!(step.run.is_empty());
        let step = plan_step(
            &[SlotView {
                schedule: &row,
                next: Some(next),
                in_flight: false,
            }],
            at,
        );
        assert_eq!(step.stop_until, vec![row.id]);
        assert!(step.run.is_empty());
    }

    #[test]
    fn plan_step_skips_a_due_slot_in_flight() {
        let row = row();
        let now = "2026-10-19T10:10:00Z".parse().unwrap();
        let step = plan_step(
            &[SlotView {
                schedule: &row,
                next: Some(now),
                in_flight: true,
            }],
            now,
        );
        assert_eq!(step.skip_in_flight, vec![row.id]);
        assert!(step.run.is_empty());
        assert_eq!(step.wake, Some("2026-10-19T11:10:00Z".parse().unwrap()));
    }

    #[test]
    fn plan_step_skips_missed_occurrences_and_stops_at_until() {
        let mut row = row();
        row.cron_expr = "0 * * * * *".into();
        row.jitter_s = 0;
        let now = "2026-10-19T03:00:00Z".parse().unwrap();
        let step = plan_step(
            &[SlotView {
                schedule: &row,
                next: Some(row.created_at.into_inner()),
                in_flight: false,
            }],
            now,
        );
        assert_eq!(step.run, vec![row.id]);
        assert_eq!(step.wake, Some("2026-10-19T03:01:00Z".parse().unwrap()));
        let at = (row.created_at.into_inner() + Duration::seconds(90)).into();
        row.stop = StopCondition::Until { at };
        for offset in [90, 120] {
            let step = plan_step(
                &[SlotView {
                    schedule: &row,
                    next: Some(row.created_at.into_inner() + Duration::seconds(60)),
                    in_flight: false,
                }],
                row.created_at.into_inner() + Duration::seconds(offset),
            );
            assert!(step.run.is_empty());
            assert_eq!(step.stop_until, vec![row.id]);
        }
    }
}
