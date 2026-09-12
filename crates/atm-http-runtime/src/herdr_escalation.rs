//! Shared lead, recipient, and Herdr notification escalation behavior.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::TaskStore;
use atm_core::boundary::{
    AsyncMailboxReader, MAX_ESCALATION_RECIPIENTS, MailboxScope, MemberKey, MessageQuery,
    ReadDeadline, ReadLaneError,
};
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::send::{NudgeMode, SendMessageSource, WriteRequest, write_mail_with_runtime};
use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};

use crate::herdr_queue_wake::run_blocking;
use crate::herdr_task_disposition::EpisodeKind;

pub(crate) const HERDR_NOTIFY_DEADLINE: Duration = Duration::from_secs(5);
pub(crate) const ESCALATION_RECIPIENT_CAP: usize = MAX_ESCALATION_RECIPIENTS;

/// Typed daemon identity for escalation mail; the constant is a valid agent
/// name, so it is constructed once instead of reparsed for every write.
static DAEMON_ACTOR: LazyLock<AgentName> =
    LazyLock::new(|| AgentName::from_validated(atm_core::boundary::DAEMON_ACTOR_NAME));

#[derive(Debug, Clone, Copy)]
pub(crate) enum EscalationKind {
    TaskStalled,
    BlockedEscalated,
    OfflineEscalated,
    RefusalsEscalated,
}

impl EscalationKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::TaskStalled => "lead_notified",
            Self::BlockedEscalated => "blocked_escalated",
            Self::OfflineEscalated => "offline_escalated",
            Self::RefusalsEscalated => "refusals_escalated",
        }
    }
}

impl From<EpisodeKind> for EscalationKind {
    fn from(kind: EpisodeKind) -> Self {
        match kind {
            EpisodeKind::Blocked => Self::BlockedEscalated,
            EpisodeKind::Offline => Self::OfflineEscalated,
        }
    }
}

/// Pump-owned blocked episode state. Keeping this state with the escalation
/// policy prevents the queue-wake file from becoming the owner of D6 data.
#[derive(Clone, Default)]
pub(crate) struct EscalationState {
    /// The process-local view of currently-open Blocked/Offline episodes.
    /// Durable duplicate suppression lives in each escalation target's mailbox.
    episodes: Arc<Mutex<HashMap<MemberKey, EpisodeKind>>>,
}

impl EscalationState {
    /// Records an episode transition. Recovery clears the local episode entry.
    pub(crate) fn observe(
        &self,
        member: &MemberKey,
        state: atm_core::protocol::RuntimeMemberState,
    ) -> bool {
        let episode = match state {
            atm_core::protocol::RuntimeMemberState::Blocked => EpisodeKind::Blocked,
            atm_core::protocol::RuntimeMemberState::Offline => EpisodeKind::Offline,
            _ => {
                self.episodes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(member);
                return false;
            }
        };
        let mut episodes = self
            .episodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        episodes.insert(member.clone(), episode) != Some(episode)
    }
}

/// The durable key shared by all escalation writers and mailbox suppression.
pub(crate) fn escalation_summary(
    kind: EscalationKind,
    member: &MemberKey,
    task: Option<&TaskId>,
) -> String {
    match task {
        Some(task_id) => format!("escalation:{}:{}:{}", kind.as_str(), member, task_id),
        None => format!("escalation:{}:{}", kind.as_str(), member),
    }
}

/// Returns whether this target already holds this episode's escalation mail.
pub(crate) async fn episode_already_reported(
    reader: &dyn AsyncMailboxReader,
    target: &MailboxScope,
    summary: &str,
    since: IsoTimestamp,
    deadline: ReadDeadline,
) -> Result<bool, ReadLaneError> {
    let query = MessageQuery {
        team: target.team.clone(),
        agent: target.agent.clone(),
        sender: Some(DAEMON_ACTOR.clone()),
        task_id: None,
        limit: None,
    };
    let messages = reader
        .list_messages(target.clone(), query, deadline)
        .await?;
    Ok(messages.iter().any(|message| {
        message.envelope.summary.as_deref() == Some(summary) && message.envelope.timestamp >= since
    }))
}

/// Writes escalation mail to the lead and configured recipients. When an
/// episode timestamp is supplied, local mailboxes suppress duplicate writes.
#[expect(
    clippy::too_many_arguments,
    reason = "the escalation boundary keeps routing and durable-suppression context explicit"
)]
pub(crate) async fn escalate_mail(
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    daemon_home: &Path,
    team: &TeamName,
    summary: &str,
    mail_body: &str,
    kind: EscalationKind,
    suppress_since: Option<IsoTimestamp>,
) -> EscalationOutcome {
    let targets = match load_escalation_targets(runtime, task_store, team).await {
        Ok(targets) => targets,
        Err(error) => {
            log_target_load_error(team, &error);
            return EscalationOutcome::default();
        }
    };
    let mut outcome = EscalationOutcome {
        lead: targets.lead.clone(),
        ..Default::default()
    };
    let reader = suppress_since.and_then(|_| match runtime.async_mailbox_reader() {
        Ok(reader) => Some(reader),
        Err(error) => {
            tracing::warn!(
                subsystem = "herdr_queue_wake",
                action = "escalation_mail_reader",
                outcome = "unavailable",
                error = %error,
                "Escalation mailbox suppression is unavailable"
            );
            None
        }
    });
    let mut recipients = targets.recipients;
    if let Some(lead) = targets.lead {
        recipients.insert(
            0,
            format!("{lead}@{team}")
                .parse()
                .expect("validated lead and team form a valid local address"),
        );
    }
    for recipient in recipients {
        if should_suppress(reader.as_deref(), &recipient, team, summary, suppress_since).await {
            continue;
        }
        match write_escalation_mail_with_summary(
            runtime,
            daemon_home,
            team,
            &recipient,
            mail_body,
            summary,
        )
        .await
        {
            Ok(message_id)
                if outcome.lead_write.is_none()
                    && recipient.to_string()
                        == outcome
                            .lead
                            .as_ref()
                            .map(|lead| format!("{lead}@{team}"))
                            .unwrap_or_default() =>
            {
                outcome.lead_write = Some(message_id)
            }
            Ok(_) => outcome.recipients_written = outcome.recipients_written.saturating_add(1),
            Err(error) => {
                outcome.recipients_failed = outcome.recipients_failed.saturating_add(1);
                tracing::warn!(subsystem = "herdr_queue_wake", action = "escalation_mail_write", outcome = "failed", kind = kind.as_str(), recipient = %recipient, error = %error, "Escalation mail write failed");
            }
        }
    }
    outcome
}

fn log_target_load_error(team: &TeamName, error: &AtmError) {
    tracing::warn!(
        subsystem = "herdr_queue_wake",
        action = "escalation_target_load",
        outcome = "failed",
        team = %team,
        error = %error,
        "Escalation target load failed"
    );
}

async fn should_suppress(
    reader: Option<&(dyn AsyncMailboxReader + Send + Sync)>,
    address: &atm_core::address::AgentAddress,
    team: &TeamName,
    summary: &str,
    since: Option<IsoTimestamp>,
) -> bool {
    let (Some(reader), Some(since)) = (reader, since) else {
        return false;
    };
    if address.host().is_some() {
        return false;
    }
    let scope = MailboxScope::new(
        address.team().cloned().unwrap_or_else(|| team.clone()),
        address.agent().clone(),
    );
    let Ok(deadline) = ReadDeadline::new(HERDR_NOTIFY_DEADLINE) else {
        return false;
    };
    match episode_already_reported(reader, &scope, summary, since, deadline).await {
        Ok(reported) => reported,
        Err(error) => {
            tracing::warn!(subsystem = "herdr_queue_wake", action = "escalation_mail_read", outcome = "failed", recipient = %address, error = %error, "Escalation mailbox suppression read failed");
            false
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EscalationOutcome {
    pub lead: Option<AgentName>,
    pub lead_write: Option<atm_core::schema::AtmMessageId>,
    pub recipients_written: u32,
    pub recipients_failed: u32,
}

impl EscalationOutcome {
    #[must_use]
    pub fn reached_anyone(&self) -> bool {
        self.lead_write.is_some() || self.recipients_written > 0
    }
}

struct EscalationTargets {
    lead: Option<AgentName>,
    recipients: Vec<atm_core::address::AgentAddress>,
}

async fn load_escalation_targets(
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
) -> Result<EscalationTargets, AtmError> {
    let roster_store = runtime.shared_roster_store_arc();
    let roster = run_blocking({
        let roster_store = Arc::clone(&roster_store);
        let team = team.clone();
        move || roster_store.load_roster(&team)
    })
    .await?;
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
) -> Vec<atm_core::address::AgentAddress> {
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

async fn write_escalation_mail_with_summary(
    runtime: &LocalServiceRuntime,
    daemon_home: &Path,
    team: &TeamName,
    recipient: &atm_core::address::AgentAddress,
    body: &str,
    summary: &str,
) -> Result<atm_core::schema::AtmMessageId, AtmError> {
    let runtime = runtime.clone();
    let body = body.to_owned();
    let daemon_home = daemon_home.to_path_buf();
    let recipient = recipient.to_string();
    let team = team.clone();
    let summary = summary.to_owned();
    run_blocking(move || {
        let request = WriteRequest::new(
            daemon_home.clone(),
            daemon_home,
            DAEMON_ACTOR.clone(),
            &recipient,
            team,
            SendMessageSource::Inline(body),
            Some(summary),
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

#[cfg(test)]
mod tests {
    use super::{EscalationKind, EscalationState, escalation_summary};
    use atm_core::boundary::MemberKey;
    use atm_core::protocol::RuntimeMemberState;

    fn member() -> MemberKey {
        MemberKey::new(
            "test-team".parse().expect("team"),
            "member".parse().expect("agent"),
        )
    }

    #[test]
    fn episode_map_replaces_kind_on_flip_and_clears_on_recovery() {
        let state = EscalationState::default();
        let member = member();
        assert!(state.observe(&member, RuntimeMemberState::Blocked));
        assert!(!state.observe(&member, RuntimeMemberState::Blocked));
        assert!(state.observe(&member, RuntimeMemberState::Offline));
        assert!(!state.observe(&member, RuntimeMemberState::Idle));
        assert!(state.observe(&member, RuntimeMemberState::Blocked));
    }

    #[test]
    fn escalation_summary_is_stable() {
        let member = member();
        let first = escalation_summary(EscalationKind::BlockedEscalated, &member, None);
        assert_eq!(
            first,
            escalation_summary(EscalationKind::BlockedEscalated, &member, None)
        );
        assert_eq!(first, "escalation:blocked_escalated:member@test-team");
    }
}
