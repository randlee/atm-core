use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

use atm_core::boundary::{CronSchedule, CronScheduleId, StopCondition};
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
    pub wake: Option<DateTime<Utc>>,
}

fn next_time(schedule: &CronSchedule, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let jitter = Duration::seconds(i64::from(schedule.jitter_s));
    let base = now.checked_sub_signed(jitter)?;
    expr::parse(&schedule.cron_expr, base)
        .ok()?
        .after(&base)
        .next()?
        .checked_add_signed(jitter)
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
            next_time(slot.schedule, now)
        } else {
            slot.next
        };
        let wake = match slot.schedule.stop {
            StopCondition::Until { at } => Some(next.map_or(at, |next| next.min(at))),
            StopCondition::Hook => next,
        };
        if let Some(wake) = wake {
            step.wake = Some(step.wake.map_or(wake, |old| old.min(wake)));
        }
    }
    step
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
            let mut slot = previous.remove(index);
            slot.next = next_time(&schedule, cron.deps.clock.now());
            slots.push(slot);
        } else {
            let next = next_time(&schedule, cron.deps.clock.now());
            if next.is_none() {
                tick::record(
                    cron,
                    &schedule,
                    &CronRunOutcome::Stopped("no future occurrence".into()),
                )
                .await;
            } else {
                slots.push(Slot {
                    schedule,
                    next,
                    in_flight: None,
                });
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

async fn reload_slots(cron: &AtmCron, slots: &mut Vec<Slot>) -> bool {
    match store_call(cron.deps.store.clone(), |store| store.active_all()).await {
        Ok(rows) => {
            merge(cron, slots, rows).await;
            false
        }
        Err(error) => {
            tracing::error!(subsystem = "atm_cron", action = "reload_failed", %error, "cron reload failed: {error}");
            true
        }
    }
}

async fn apply_step(cron: &Arc<AtmCron>, slots: &mut Vec<Slot>, running: &watch::Sender<usize>) {
    let now = cron.deps.clock.now();
    let step = step_for(slots, now);
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
    for slot in slots {
        if step.skip_in_flight.contains(&slot.schedule.id) {
            tick::log(&slot.schedule, "tick", &CronRunOutcome::SkippedInFlight, 0);
            slot.next = next_time(&slot.schedule, now);
        } else if step.run.contains(&slot.schedule.id) {
            let owner = cron.clone();
            let schedule = slot.schedule.clone();
            let guard = Running::new(running.clone());
            slot.in_flight = Some(tokio::spawn(TrackedRun {
                future: Box::pin(async move { tick::run_one(&owner, &schedule).await }),
                _guard: guard,
            }));
            slot.next = next_time(&slot.schedule, now);
        }
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
    let mut retry_reload = false;
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
        tokio::select! {
            biased;
            result = reload.changed() => {
                if result.is_err() { break; }
                retry_reload = reload_slots(&cron, &mut slots).await;
            }
            () = cron.deps.clock.sleep_until(wake) => {
                if retry_reload { retry_reload = reload_slots(&cron, &mut slots).await; }
                apply_step(&cron, &mut slots, &running).await;
            }
            (index, result) = finished(&mut slots) => {
                slots[index].in_flight.take();
                if complete(&cron, &slots[index], result).await { slots.remove(index); }
                else { slots[index].next = next_time(&slots[index].schedule, cron.deps.clock.now()); }
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

    fn row() -> CronSchedule {
        serde_json::from_value(serde_json::json!({
            "id":1,"generation":1,"team":"test-team","phase":"bk","name":"test",
            "cron_expr":"@hourly","module_path":"/module.py","cwd":"/work","args":[],
            "timeout_ms":1000,"jitter_s":600,"custom":{},"stop":{"kind":"hook"},
            "active":true,"created_by":"solar","created_at":"2026-10-19T00:00:00Z",
            "last_run_at":null,"last_outcome":null,"ended_at":null
        }))
        .unwrap()
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
        let at = "2026-10-19T10:05:00Z".parse().unwrap();
        row.stop = StopCondition::Until { at };
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
        let at = row.created_at.into_inner() + Duration::seconds(90);
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
