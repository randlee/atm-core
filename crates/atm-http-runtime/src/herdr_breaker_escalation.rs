//! Pump-owned breaker-cycle escalation admission.
//!
//! This state deliberately lives in the Tokio runtime rather than `atm-herdr`:
//! the adapter owns only the shared process breaker, while the runtime owns
//! durable-mail policy and its bounded notification side effect.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::TaskStore;
use atm_core::types::IsoTimestamp;
use atm_core::types::TeamName;
use atm_herdr::HerdrProcessAdapter;

use crate::herdr_escalation::{EscalationKind, EscalationNotification, escalate};

/// In-memory admission gate for one Herdr endpoint's breaker cycles.
#[derive(Debug)]
pub(crate) struct HerdrBreakerEscalationGate {
    last_escalated_opened_at: Option<IsoTimestamp>,
    last_escalated_at: Option<IsoTimestamp>,
    min_interval: Duration,
}

impl HerdrBreakerEscalationGate {
    #[must_use]
    pub(crate) const fn new(min_interval: Duration) -> Self {
        Self {
            last_escalated_opened_at: None,
            last_escalated_at: None,
            min_interval,
        }
    }

    /// Claims a distinct open cycle only when its configured cooldown has
    /// elapsed. Suppression is observable without retaining any mail data.
    pub(crate) fn claim(&mut self, opened_at: IsoTimestamp, now: IsoTimestamp) -> bool {
        if self.last_escalated_opened_at == Some(opened_at) {
            tracing::info!(
                event = "herdr_breaker_escalation",
                outcome = "suppressed_same_cycle",
                breaker_cycle = %opened_at,
                "Herdr breaker escalation was already admitted for this cycle"
            );
            return false;
        }
        if self
            .last_escalated_at
            .is_some_and(|last| elapsed(now, last) < self.min_interval)
        {
            tracing::info!(
                event = "herdr_breaker_escalation",
                outcome = "suppressed_min_interval",
                breaker_cycle = %opened_at,
                min_interval_secs = self.min_interval.as_secs(),
                "Herdr breaker escalation is not yet eligible"
            );
            return false;
        }
        self.last_escalated_opened_at = Some(opened_at);
        self.last_escalated_at = Some(now);
        true
    }
}

fn elapsed(now: IsoTimestamp, then: IsoTimestamp) -> Duration {
    now.into_inner()
        .signed_duration_since(then.into_inner())
        .to_std()
        .unwrap_or_default()
}

/// Performs a single admitted breaker-cycle escalation. Durable mail and the
/// desktop notification intentionally use distinct, privacy-safe payloads.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn escalate_breaker_cycle(
    runtime: &LocalServiceRuntime,
    herdr_process: &dyn HerdrProcessAdapter,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    daemon_home: &Path,
    team: &TeamName,
    opened_at: IsoTimestamp,
) {
    let mail_body = format!(
        "Herdr breaker opened at {opened_at}. Queued ATM mail remains durable. Remediation: run atm doctor --json."
    );
    let notification = EscalationNotification {
        title: "ATM Herdr breaker open".to_owned(),
        body: "state=breaker_open remediation=atm doctor --json".to_owned(),
    };
    let outcome = escalate(
        runtime,
        herdr_process,
        task_store,
        daemon_home,
        team,
        &mail_body,
        &notification,
        EscalationKind::BreakerOpened,
    )
    .await;
    tracing::info!(
        event = "herdr_breaker_escalation",
        outcome = "completed",
        breaker_cycle = %opened_at,
        lead_write = outcome.lead_write.is_some(),
        recipients_written = outcome.recipients_written,
        recipients_failed = outcome.recipients_failed,
        notify_ok = outcome.notify_ok,
        "Herdr breaker escalation completed"
    );
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::time::Duration;

    use atm_core::types::IsoTimestamp;

    use super::HerdrBreakerEscalationGate;

    fn timestamp(value: &str) -> IsoTimestamp {
        IsoTimestamp::from_str(value).expect("valid test timestamp")
    }

    #[test]
    fn claims_once_per_cycle_and_after_the_interval_for_a_new_cycle() {
        let mut gate = HerdrBreakerEscalationGate::new(Duration::from_secs(30));
        let first = timestamp("2030-01-01T00:00:00Z");
        let second = timestamp("2030-01-01T00:00:01Z");

        assert!(gate.claim(first, first));
        assert!(!gate.claim(first, timestamp("2030-01-01T00:00:30Z")));
        assert!(!gate.claim(second, second));
        assert!(gate.claim(second, timestamp("2030-01-01T00:00:30Z")));
    }
}
