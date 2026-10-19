use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[async_trait]
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
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
}

#[cfg(feature = "test-utils")]
impl ManualClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        Self {
            now: tokio::sync::watch::channel(start).0,
            parks: tokio::sync::watch::channel(0).0,
        }
    }

    pub fn advance(&self, duration: chrono::Duration) {
        self.now.send_modify(|now| *now += duration);
    }

    pub fn set(&self, time: DateTime<Utc>) {
        self.now.send_replace(time);
    }

    pub fn parks(&self) -> u64 {
        *self.parks.borrow()
    }

    pub async fn wait_parks_above(&self, count: u64) {
        self.parks
            .subscribe()
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
