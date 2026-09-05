//! Shared lead, recipient, and Herdr notification escalation behavior.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::api::RequestDeadline;
use atm_core::boundary::TaskStore;
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, TeamName};
use atm_herdr::HerdrProcessAdapter;

use crate::herdr_queue_wake::run_blocking;

pub(crate) const HERDR_NOTIFY_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const ESCALATION_RECIPIENT_CAP: usize = 8;
pub(crate) const BLOCKED_NOTIFY_MS: u64 = 60_000;
pub(crate) const BLOCKED_RENOTIFY_MS: u64 = 600_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EscalationOutcome {
    pub lead: Option<AgentName>,
    pub lead_write: Option<atm_core::schema::AtmMessageId>,
    pub recipients_written: u32,
    pub recipients_failed: u32,
    pub notify_ok: bool,
}

impl EscalationOutcome {
    #[must_use]
    pub fn reached_anyone(&self) -> bool {
        self.lead_write.is_some() || self.recipients_written > 0 || self.notify_ok
    }
}

/// Delivers one escalation to the lead, configured recipients, and Herdr.
/// Mail is deliberately written through the same canonical deferred path as
/// `atm send`; the caller supplies the pump's blocking helper for every write.
pub(crate) async fn escalate(
    runtime: &LocalServiceRuntime,
    herdr_process: &dyn HerdrProcessAdapter,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    daemon_home: &Path,
    team: &TeamName,
    body: &str,
    kind: &str,
) -> EscalationOutcome {
    let targets = match load_escalation_targets(runtime, task_store, team).await {
        Ok(targets) => targets,
        Err(()) => return notify_only(herdr_process, team, body, kind).await,
    };
    let mut outcome = EscalationOutcome {
        lead: targets.lead,
        ..Default::default()
    };
    write_target_mail(
        runtime,
        daemon_home,
        team,
        body,
        targets.recipients,
        &mut outcome,
    )
    .await;
    outcome.notify_ok = notify(herdr_process, body).await;
    tracing::info!(
        event = "herdr_queue_poll_outcome",
        subsystem = "herdr_queue_wake",
        action = "escalation",
        outcome = kind,
        team = %team,
        lead_present = outcome.lead.is_some(),
        recipients_written = outcome.recipients_written,
        recipients_failed = outcome.recipients_failed,
        notify_ok = outcome.notify_ok,
        "Herdr escalation completed"
    );
    outcome
}

struct EscalationTargets {
    lead: Option<AgentName>,
    recipients: Vec<String>,
}

async fn load_escalation_targets(
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
) -> Result<EscalationTargets, ()> {
    let roster_store = runtime.shared_roster_store_arc();
    let roster = match run_blocking({
        let roster_store = Arc::clone(&roster_store);
        let team = team.clone();
        move || roster_store.load_roster(&team)
    })
    .await
    {
        Ok(roster) => roster,
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "escalation_roster_read",
                outcome = "failed",
                team = %team,
                error = %error,
                "Escalation roster read failed"
            );
            return Err(());
        }
    };
    let leads: Vec<_> = roster
        .members
        .iter()
        .filter(|member| member.agent_type == atm_core::schema::AgentType::Lead)
        .map(|member| member.agent_name.clone())
        .collect();
    let lead = (leads.len() == 1).then(|| leads[0].clone());
    let recipients = load_escalation_recipients(task_store, team).await;
    Ok(EscalationTargets { lead, recipients })
}

async fn load_escalation_recipients(
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
) -> Vec<String> {
    let recipients = match task_store {
        Some(store) => match run_blocking({
            let store = Arc::clone(store);
            let team = team.clone();
            move || store.effective_escalation_recipients(&team)
        })
        .await
        {
            Ok(recipients) => recipients,
            Err(error) => {
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "escalation_recipient_read",
                    outcome = "failed",
                    team = %team,
                    error = %error,
                    "Escalation recipient read failed"
                );
                Vec::new()
            }
        },
        None => Vec::new(),
    };
    if recipients.len() > ESCALATION_RECIPIENT_CAP {
        tracing::warn!(
            subsystem = "herdr_queue_wake",
            action = "escalation_recipient_cap",
            outcome = "capped",
            team = %team,
            configured = recipients.len(),
            cap = ESCALATION_RECIPIENT_CAP,
            "Escalation recipient list capped for this tick"
        );
        recipients[..ESCALATION_RECIPIENT_CAP].to_vec()
    } else {
        recipients
    }
}

async fn write_target_mail(
    runtime: &LocalServiceRuntime,
    daemon_home: &Path,
    team: &TeamName,
    body: &str,
    recipients: Vec<String>,
    outcome: &mut EscalationOutcome,
) {
    if let Some(lead) = outcome.lead.clone() {
        let address = format!("{lead}@{team}");
        match write_escalation_mail(runtime, daemon_home, team, &address, body).await {
            Ok(message_id) => outcome.lead_write = Some(message_id),
            Err(error) => tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "escalation_lead_write",
                outcome = "failed",
                team = %team,
                lead = %lead,
                error = %error,
                "Escalation lead mail write failed"
            ),
        }
    }
    for recipient in recipients {
        match write_escalation_mail(runtime, daemon_home, team, &recipient, body).await {
            Ok(_) => outcome.recipients_written = outcome.recipients_written.saturating_add(1),
            Err(error) => {
                outcome.recipients_failed = outcome.recipients_failed.saturating_add(1);
                tracing::warn!(
                    subsystem = "herdr_queue_wake",
                    action = "escalation_recipient_write",
                    outcome = "failed",
                    team = %team,
                    recipient = %recipient,
                    error = %error,
                    "Escalation recipient mail write failed"
                );
            }
        }
    }
}

async fn notify_only(
    herdr_process: &dyn HerdrProcessAdapter,
    team: &TeamName,
    body: &str,
    kind: &str,
) -> EscalationOutcome {
    let notify_ok = notify(herdr_process, body).await;
    tracing::info!(
        event = "herdr_queue_poll_outcome",
        subsystem = "herdr_queue_wake",
        action = "escalation",
        outcome = kind,
        team = %team,
        lead_present = false,
        recipients_written = 0,
        recipients_failed = 0,
        notify_ok,
        "Herdr escalation completed without roster data"
    );
    EscalationOutcome {
        notify_ok,
        ..Default::default()
    }
}

async fn notify(herdr_process: &dyn HerdrProcessAdapter, body: &str) -> bool {
    let (title, notification_body) = body.split_once('\n').map_or((body, ""), |parts| parts);
    match herdr_process
        .notify(
            title,
            notification_body,
            RequestDeadline::after(HERDR_NOTIFY_DEADLINE),
        )
        .await
    {
        Ok(()) => true,
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "herdr_notify",
                outcome = "failed",
                error = ?error,
                "Herdr escalation notification failed"
            );
            false
        }
    }
}

async fn write_escalation_mail(
    runtime: &LocalServiceRuntime,
    daemon_home: &Path,
    team: &TeamName,
    recipient: &str,
    body: &str,
) -> Result<atm_core::schema::AtmMessageId, AtmError> {
    let runtime = runtime.clone();
    let body = body.to_owned();
    let daemon_home = daemon_home.to_path_buf();
    let recipient = recipient.to_owned();
    let team = team.clone();
    run_blocking(move || {
        let request = WriteRequest::new(
            daemon_home.clone(),
            daemon_home,
            atm_core::boundary::DAEMON_ACTOR_NAME
                .parse()
                .map_err(|error| {
                    AtmError::validation(format!("invalid daemon actor name: {error}"))
                })?,
            &recipient,
            team,
            SendMessageSource::Inline(body),
            None,
            false,
            None,
            false,
        )?
        .with_nudge_mode(NudgeMode::Deferred);
        write_mail_with_runtime(request, &NullObservability, &runtime)
            .map(|outcome| outcome.persisted_message_id())
    })
    .await
}
