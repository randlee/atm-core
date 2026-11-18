use std::time::Duration;

use async_trait::async_trait;
use atm_core::api::{RequestBudget, RequestDeadline};
use chrono::{DateTime, Utc};

#[async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
    fn monotonic_now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }
    fn request_deadline(&self, budget: RequestBudget) -> RequestDeadline {
        budget.start_at(self.monotonic_now())
    }
    async fn sleep_until(&self, target: DateTime<Utc>);
}

pub struct SystemClock;

pub(crate) const MAX_SLEEP_SLICE: Duration = Duration::from_secs(30);

pub(crate) fn system_sleep_step(now: DateTime<Utc>, target: DateTime<Utc>) -> Duration {
    (target - now)
        .to_std()
        .unwrap_or_default()
        .min(MAX_SLEEP_SLICE)
}

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }

    #[sc_lint_attributes::sc_lint(allow(SCB-RUNTIME-005), reason = "The scheduler waits until its target wall-clock time, rechecking after bounded clock steps.")]
    async fn sleep_until(&self, target: DateTime<Utc>) {
        loop {
            let duration = system_sleep_step(self.now(), target);
            if duration.is_zero() {
                return;
            }
            tokio::time::sleep(duration).await;
        }
    }
}

#[cfg(feature = "test-utils")]
pub struct ManualClock {
    now: tokio::sync::watch::Sender<DateTime<Utc>>,
    parks: tokio::sync::watch::Sender<u64>,
    monotonic: std::sync::Mutex<std::time::Instant>,
    unbounded_deadlines: bool,
}

#[cfg(feature = "test-utils")]
impl ManualClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self {
            now: tokio::sync::watch::channel(start).0,
            parks: tokio::sync::watch::channel(0).0,
            monotonic: std::sync::Mutex::new(std::time::Instant::now()),
            unbounded_deadlines: false,
        }
    }

    pub fn with_unbounded_deadlines(mut self) -> Self {
        self.unbounded_deadlines = true;
        self
    }

    pub fn advance(&self, duration: chrono::Duration) {
        self.advance_monotonic(duration);
        self.now.send_modify(|now| *now += duration);
    }

    pub fn set(&self, time: DateTime<Utc>) {
        self.advance_monotonic(time - self.now());
        self.now.send_replace(time);
    }

    fn advance_monotonic(&self, duration: chrono::Duration) {
        *self
            .monotonic
            .lock()
            .expect("manual monotonic clock poisoned") += duration.to_std().unwrap_or_default();
    }

    pub fn parks(&self) -> u64 {
        *self.parks.borrow()
    }

    pub async fn wait_parks_above(&self, count: u64) {
        let mut parks = self.parks.subscribe();
        parks
            .wait_for(|value| *value > count)
            .await
            .expect("manual clock sender alive");
    }

    pub async fn advance_settled(&self, duration: chrono::Duration) {
        let count = self.parks();
        self.advance(duration);
        self.wait_parks_above(count).await;
    }
}

#[cfg(feature = "test-utils")]
#[async_trait]
impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.borrow()
    }

    fn monotonic_now(&self) -> std::time::Instant {
        *self
            .monotonic
            .lock()
            .expect("manual monotonic clock poisoned")
    }

    fn request_deadline(&self, budget: RequestBudget) -> RequestDeadline {
        if self.unbounded_deadlines {
            RequestDeadline::unbounded()
        } else {
            budget.start_at(self.monotonic_now())
        }
    }

    async fn sleep_until(&self, target: DateTime<Utc>) {
        let mut receiver = self.now.subscribe();
        loop {
            if *receiver.borrow_and_update() >= target {
                return;
            }
            self.parks.send_modify(|count| *count += 1);
            receiver.changed().await.expect("manual clock sender alive");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "test-utils")]
    #[test]
    fn manual_monotonic_time_moves_only_with_controlled_positive_steps() {
        let clock = ManualClock::new("2026-10-19T00:00:00Z".parse().unwrap());
        let initial = clock.monotonic_now();
        assert_eq!(clock.monotonic_now(), initial);
        clock.advance(chrono::Duration::seconds(60));
        assert_eq!(clock.monotonic_now(), initial + Duration::from_secs(60));
        clock.set(clock.now() - chrono::Duration::seconds(30));
        assert_eq!(clock.monotonic_now(), initial + Duration::from_secs(60));
        clock.set(clock.now() + chrono::Duration::seconds(10));
        assert_eq!(clock.monotonic_now(), initial + Duration::from_secs(70));
    }

    #[test]
    fn system_sleep_step_is_capped_at_the_slice() {
        let now = "2026-10-19T00:00:00Z".parse().unwrap();
        assert_eq!(
            system_sleep_step(now, now + chrono::Duration::hours(1)),
            MAX_SLEEP_SLICE
        );
        assert_eq!(
            system_sleep_step(now, now + chrono::Duration::seconds(10)),
            Duration::from_secs(10)
        );
        assert_eq!(
            system_sleep_step(now, now - chrono::Duration::seconds(1)),
            Duration::ZERO
        );
    }
}
