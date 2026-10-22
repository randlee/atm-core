//! Shared lead, recipient, and Herdr notification escalation behavior.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use atm_core::LocalServiceRuntime;
use atm_core::boundary::TaskStore;
use atm_core::boundary::{
    AsyncMailboxReader, MAX_ESCALATION_RECIPIENTS, MailboxScope, MemberKey, MessageQuery,
    ReadDeadline, ReadLaneError, TaskRow,
};
use atm_core::error::AtmError;
use atm_core::observability::NullObservability;
use atm_core::send::{
    NudgeMode, SendMessageSource, WriteRequest,
    write_current_herdr_terminal_escalation_with_runtime, write_mail_with_runtime,
};
use atm_core::types::{AgentName, IsoTimestamp, TaskId, TeamName};

use crate::herdr_queue_wake::herdr_request_deadline;
use crate::herdr_task_disposition::EpisodeKind;
use crate::router_support::BoundedBlockingBridge;

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
#[derive(Clone, Copy, PartialEq, Eq)]
struct Episode {
    kind: EpisodeKind,
    since: Option<IsoTimestamp>,
}

#[derive(Clone, Default)]
pub(crate) struct EscalationState {
    /// The process-local view of currently-open Blocked/Offline episodes.
    /// Durable duplicate suppression lives in each escalation target's mailbox.
    episodes: Arc<Mutex<HashMap<MemberKey, Episode>>>,
}

pub(crate) enum EpisodeObservation {
    Transition,
    Verify {
        kind: EpisodeKind,
        since: IsoTimestamp,
    },
}

impl EscalationState {
    /// Records an episode transition or verifies a fresh observation. Recovery
    /// clears the local episode entry only on the transition path.
    pub(crate) fn observe(
        &self,
        member: &MemberKey,
        state: atm_core::protocol::RuntimeMemberState,
        state_changed_at: Option<IsoTimestamp>,
        observation: EpisodeObservation,
    ) -> bool {
        let episode = match state {
            atm_core::protocol::RuntimeMemberState::Blocked => EpisodeKind::Blocked,
            atm_core::protocol::RuntimeMemberState::Offline => EpisodeKind::Offline,
            _ => {
                if matches!(observation, EpisodeObservation::Transition) {
                    self.episodes
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(member);
                }
                return false;
            }
        };
        match observation {
            EpisodeObservation::Transition => {
                self.episodes
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        member.clone(),
                        Episode {
                            kind: episode,
                            since: state_changed_at,
                        },
                    )
                    != Some(Episode {
                        kind: episode,
                        since: state_changed_at,
                    })
            }
            EpisodeObservation::Verify { kind, since } => {
                episode == kind && state_changed_at == Some(since)
            }
        }
    }

    pub(crate) fn hold_targets_unavailable(&self, member: &MemberKey) {
        self.episodes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(member);
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
    blocking_bridge: &BoundedBlockingBridge,
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    source_task: &TaskRow,
    episode: Option<(atm_core::protocol::RuntimeMemberState, IsoTimestamp)>,
    daemon_home: &Path,
    team: &TeamName,
    summary: &str,
    mail_body: &str,
    kind: EscalationKind,
    suppress_since: Option<IsoTimestamp>,
) -> Result<EscalationOutcome, AtmError> {
    if episode.is_some() {
        if terminal_recipients_are_suppressed(
            blocking_bridge,
            runtime,
            task_store,
            team,
            summary,
            suppress_since,
        )
        .await?
        {
            return Ok(EscalationOutcome::default());
        }
        return write_terminal_episode(blocking_bridge, runtime, source_task, daemon_home).await;
    }
    let targets = load_escalation_targets(blocking_bridge, runtime, task_store, team)
        .await
        .inspect_err(|error| log_target_load_error(team, error))?;
    let context = EscalationWriteContext {
        daemon_home,
        team,
        summary,
        mail_body,
        kind,
        suppress_since,
    };
    write_to_escalation_recipients(blocking_bridge, runtime, &context, targets).await
}

async fn terminal_recipients_are_suppressed(
    blocking_bridge: &BoundedBlockingBridge,
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
    summary: &str,
    suppress_since: Option<IsoTimestamp>,
) -> Result<bool, AtmError> {
    let targets = load_escalation_targets(blocking_bridge, runtime, task_store, team)
        .await
        .inspect_err(|error| log_target_load_error(team, error))?;
    let leads = if targets.leads.len() == 1 {
        targets.leads
    } else {
        Vec::new()
    };
    let recipients = leads
        .into_iter()
        .map(|lead| {
            format!("{lead}@{team}")
                .parse()
                .expect("validated lead and team form a valid local address")
        })
        .chain(targets.recipients);
    let reader = escalation_reader(runtime, suppress_since);
    let mut found_recipient = false;
    for recipient in recipients {
        found_recipient = true;
        if !should_suppress(reader.as_deref(), &recipient, team, summary, suppress_since).await {
            return Ok(false);
        }
    }
    Ok(found_recipient)
}

async fn write_terminal_episode(
    blocking_bridge: &BoundedBlockingBridge,
    runtime: &LocalServiceRuntime,
    source_task: &TaskRow,
    daemon_home: &Path,
) -> Result<EscalationOutcome, AtmError> {
    let runtime = runtime.clone();
    let daemon_home = daemon_home.to_path_buf();
    let source_member = MemberKey::new(source_task.team.clone(), source_task.assignee.clone());
    let admitted = blocking_bridge
        .run(herdr_request_deadline(), move || {
            write_current_herdr_terminal_escalation_with_runtime(
                daemon_home,
                &source_member,
                &NullObservability,
                &runtime,
            )
        })
        .await?;
    let recipients_written = admitted.recipients_written();
    Ok(EscalationOutcome {
        audit_delivery: admitted.deliveries.into_iter().next(),
        recipients_written,
        recipients_failed: admitted.recipients_failed,
    })
}

struct EscalationWriteContext<'a> {
    daemon_home: &'a Path,
    team: &'a TeamName,
    summary: &'a str,
    mail_body: &'a str,
    kind: EscalationKind,
    suppress_since: Option<IsoTimestamp>,
}

async fn write_to_escalation_recipients(
    blocking_bridge: &BoundedBlockingBridge,
    runtime: &LocalServiceRuntime,
    context: &EscalationWriteContext<'_>,
    targets: EscalationTargets,
) -> Result<EscalationOutcome, AtmError> {
    let reader = escalation_reader(runtime, context.suppress_since);
    let leads = match context.kind {
        EscalationKind::TaskStalled => targets.leads,
        _ if targets.leads.len() == 1 => targets.leads,
        _ => Vec::new(),
    };
    let recipients = leads
        .into_iter()
        .map(|lead| {
            format!("{lead}@{}", context.team)
                .parse()
                .expect("validated lead and team form a valid local address")
        })
        .chain(targets.recipients);
    let mut outcome = EscalationOutcome::default();
    for recipient in recipients {
        if should_suppress(
            reader.as_deref(),
            &recipient,
            context.team,
            context.summary,
            context.suppress_since,
        )
        .await
        {
            continue;
        }
        match write_escalation_mail_with_summary(
            blocking_bridge,
            runtime,
            context.daemon_home,
            context.team,
            &recipient,
            context.mail_body,
            context.summary,
        )
        .await
        {
            Ok(message_id) => {
                record_escalation_write(&mut outcome, recipient.agent().clone(), message_id)
            }
            Err(error) => {
                outcome.recipients_failed = outcome.recipients_failed.saturating_add(1);
                tracing::warn!(subsystem = "herdr_queue_wake", action = "escalation_mail_write", outcome = "failed", kind = context.kind.as_str(), recipient = %recipient, error = %error, "Escalation mail write failed");
            }
        }
    }
    Ok(outcome)
}

fn escalation_reader(
    runtime: &LocalServiceRuntime,
    suppress_since: Option<IsoTimestamp>,
) -> Option<Arc<dyn AsyncMailboxReader + Send + Sync>> {
    suppress_since.and_then(|_| runtime.async_mailbox_reader().ok())
}

fn record_escalation_write(
    outcome: &mut EscalationOutcome,
    recipient: AgentName,
    message_id: atm_core::schema::AtmMessageId,
) {
    if outcome.audit_delivery.is_none() {
        outcome.audit_delivery = Some((recipient, message_id));
    }
    outcome.recipients_written = outcome.recipients_written.saturating_add(1);
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
    pub audit_delivery: Option<(AgentName, atm_core::schema::AtmMessageId)>,
    pub recipients_written: u32,
    pub recipients_failed: u32,
}

impl EscalationOutcome {
    #[must_use]
    pub fn reached_anyone(&self) -> bool {
        self.recipients_written > 0
    }
}

struct EscalationTargets {
    leads: Vec<AgentName>,
    recipients: Vec<atm_core::address::AgentAddress>,
}

async fn load_escalation_targets(
    blocking_bridge: &BoundedBlockingBridge,
    runtime: &LocalServiceRuntime,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
) -> Result<EscalationTargets, AtmError> {
    let roster_store = runtime.shared_roster_store_arc();
    let roster = blocking_bridge
        .run(herdr_request_deadline(), {
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
    let recipients = load_escalation_recipients(blocking_bridge, task_store, team).await?;
    Ok(EscalationTargets { leads, recipients })
}

async fn load_escalation_recipients(
    blocking_bridge: &BoundedBlockingBridge,
    task_store: Option<&Arc<dyn TaskStore + Send + Sync>>,
    team: &TeamName,
) -> Result<Vec<atm_core::address::AgentAddress>, AtmError> {
    let recipients = match task_store {
        Some(store) => {
            blocking_bridge
                .run(herdr_request_deadline(), {
                    let store = Arc::clone(store);
                    let team = team.clone();
                    move || store.effective_escalation_recipients(&team)
                })
                .await?
        }
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
        Ok(recipients[..ESCALATION_RECIPIENT_CAP].to_vec())
    } else {
        Ok(recipients)
    }
}

async fn write_escalation_mail_with_summary(
    blocking_bridge: &BoundedBlockingBridge,
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
    blocking_bridge
        .run(herdr_request_deadline(), move || {
            let request = WriteRequest::new(
                daemon_home.clone(),
                daemon_home,
                AgentName::from_validated(atm_core::boundary::DAEMON_ACTOR_NAME),
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
    use super::{EpisodeObservation, EscalationKind, EscalationState, escalation_summary};
    use atm_core::boundary::MemberKey;
    use atm_core::protocol::RuntimeMemberState;
    use atm_core::types::IsoTimestamp;

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
        assert!(state.observe(
            &member,
            RuntimeMemberState::Blocked,
            None,
            EpisodeObservation::Transition,
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Blocked,
            None,
            EpisodeObservation::Transition,
        ));
        assert!(state.observe(
            &member,
            RuntimeMemberState::Offline,
            None,
            EpisodeObservation::Transition,
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Idle,
            None,
            EpisodeObservation::Transition,
        ));
        assert!(state.observe(
            &member,
            RuntimeMemberState::Blocked,
            None,
            EpisodeObservation::Transition,
        ));
    }

    #[test]
    fn episode_verification_requires_matching_kind_and_epoch() {
        let state = EscalationState::default();
        let member = member();
        let since: IsoTimestamp = "2030-01-01T00:00:00Z".parse().expect("timestamp");
        let later: IsoTimestamp = "2030-01-01T00:00:01Z".parse().expect("timestamp");

        assert!(state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(since),
            EpisodeObservation::Verify {
                kind: crate::herdr_task_disposition::EpisodeKind::Blocked,
                since,
            },
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Offline,
            Some(since),
            EpisodeObservation::Verify {
                kind: crate::herdr_task_disposition::EpisodeKind::Blocked,
                since,
            },
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(later),
            EpisodeObservation::Verify {
                kind: crate::herdr_task_disposition::EpisodeKind::Blocked,
                since,
            },
        ));
    }

    #[test]
    fn same_kind_episode_with_new_epoch_is_a_new_transition() {
        let state = EscalationState::default();
        let member = member();
        let first: IsoTimestamp = "2030-01-01T00:00:00Z".parse().expect("timestamp");
        let later: IsoTimestamp = "2030-01-01T00:00:01Z".parse().expect("timestamp");

        assert!(state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(first),
            EpisodeObservation::Transition,
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(first),
            EpisodeObservation::Transition,
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(later),
            EpisodeObservation::Verify {
                kind: crate::herdr_task_disposition::EpisodeKind::Blocked,
                since: first,
            },
        ));
        assert!(state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(later),
            EpisodeObservation::Transition,
        ));
        assert!(!state.observe(
            &member,
            RuntimeMemberState::Blocked,
            Some(later),
            EpisodeObservation::Transition,
        ));
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
