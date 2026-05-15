use std::path::PathBuf;

use crate::boundary;
use crate::config;
use crate::error::AtmError;
use crate::identity;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{
    PostSendHookContext, ResolvedRecipient, append_mailbox_message_and_seed_workflow, input,
    summary,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Map;

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub team_override: Option<TeamName>,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

/// Summary of one successful acknowledgement and reply emission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_target: ReplyTarget,
    pub reply_message_id: AtmMessageId,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
}

impl ReplyTarget {
    fn new(agent: AgentName, team: TeamName) -> Self {
        Self { agent, team }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)
    }
}

impl Serialize for ReplyTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ReplyTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let (agent, team) = value
            .split_once('@')
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        Ok(Self::new(
            agent.parse().map_err(serde::de::Error::custom)?,
            team.parse().map_err(serde::de::Error::custom)?,
        ))
    }
}

/// Acknowledge one previously read pending-ack message and append a reply.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`],
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`], or
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`] when actor or
/// team resolution fails, the message is missing or no longer pending
/// acknowledgement, reply-target validation fails, or either the source or
/// reply inbox cannot be persisted.
pub fn ack_mail(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let runtime = default_runtime()?;
    ack_mail_with_runtime(request, observability, &runtime)
}

pub fn ack_mail_with_runtime(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<AckOutcome, AtmError> {
    ack_mail_with_runtime_impl(request, observability, runtime)
}

fn ack_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<AckOutcome, AtmError> {
    let config = runtime.load_config(&request.current_dir)?;
    let actor =
        identity::resolve_actor_identity(request.actor_override.as_deref(), config.as_ref())?;
    let team = config::resolve_team(request.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = runtime.team_dir(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    let team_config = runtime.load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == actor.as_str())
    {
        return Err(AtmError::agent_not_found(&actor, &team));
    }
    ack_mail_with_runtime_sqlite(
        request,
        observability,
        runtime,
        config.as_ref(),
        actor,
        team,
    )
}

fn ack_mail_with_runtime_sqlite<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    config: Option<&crate::config::AtmConfig>,
    actor: AgentName,
    team: TeamName,
) -> Result<AckOutcome, AtmError> {
    let source_row = load_ack_source_row(runtime, &request, &team, &actor)?;
    let source_record = load_ack_source_record(runtime, &request, &team, &actor, &source_row)?;
    ensure_ack_source_state(request.message_id, &source_record.envelope)?;
    let reply_target =
        resolve_validated_reply_target(runtime, &request.home_dir, &source_record.envelope, &team)?;

    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_message_id = AtmMessageId::new();
    let source_task_id = source_record.envelope.task_id.clone();
    let reply_message = MessageEnvelope {
        from: actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp,
        read: false,
        source_team: Some(team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(request.message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    };
    persist_acknowledged_source_state(
        runtime,
        &team,
        &actor,
        &source_row.message_key,
        &source_record.envelope,
        ack_timestamp,
    )?;
    append_reply_to_target(runtime, &request.home_dir, &reply_target, &reply_message)?;

    let mut outcome = AckOutcome {
        action: CommandAction::Ack,
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        task_id: source_task_id.clone(),
        reply_target: reply_target.clone(),
        reply_message_id,
        reply_text: reply_text.clone(),
        warnings: Vec::new(),
    };
    populate_ack_warnings(
        runtime,
        &mut outcome,
        config,
        &actor,
        &team,
        reply_message_id,
        &reply_target,
    );
    if let Err(error) = observability.emit(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor,
        message_id: Some(request.message_id),
        requires_ack: false,
        dry_run: false,
        task_id: source_task_id,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(%error, command = "ack", action = "ack", "failed to emit ack command event");
    }

    Ok(outcome)
}

fn load_ack_source_row<R: RetainedMailboxRuntime>(
    runtime: &R,
    request: &AckRequest,
    team: &TeamName,
    actor: &AgentName,
) -> Result<boundary::MailStoreMailboxMetadataRow, AtmError> {
    let metadata_rows =
        runtime.query_mailbox_metadata_rows(&request.home_dir, team, actor, None)?;
    let source_row = metadata_rows
        .iter()
        .find(|row| row.message_id == Some(request.message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} was not found in {}@{}",
                request.message_id, actor, team
            ))
            .with_recovery(
                "Refresh the mailbox with `atm read` and choose a message that is still present in the pending-ack surface.",
            )
        })?;
    if metadata_rows
        .iter()
        .any(|row| row.parent_message_id == Some(request.message_id))
    {
        return Err(AtmError::validation(format!(
            "message {} has been updated; acknowledge the current terminal message instead",
            request.message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and acknowledge the latest message in the thread instead of an older parent message.",
        ));
    }
    Ok(source_row.clone())
}

fn load_ack_source_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    request: &AckRequest,
    team: &TeamName,
    actor: &AgentName,
    source_row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<boundary::MailStoreMessageRecord, AtmError> {
    runtime
        .load_message_record(&request.home_dir, team, actor, &source_row.message_key)?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} metadata could not be reloaded from sqlite",
                request.message_id
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying `atm ack`.",
            )
        })
}

fn ensure_ack_source_state(
    message_id: AtmMessageId,
    message: &MessageEnvelope,
) -> Result<(), AtmError> {
    match (
        state::derive_read_state(message),
        state::derive_ack_state(message),
    ) {
        (crate::types::ReadState::Read, crate::types::AckState::PendingAck) => Ok(()),
        (_, crate::types::AckState::Acknowledged) => Err(AtmError::validation(format!(
            "message {message_id} is already acknowledged"
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
        _ => Err(AtmError::validation(format!(
            "message {message_id} is not in the (read, pending_ack) state"
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
    }
}

fn resolve_validated_reply_target<R: RetainedServiceRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    message: &MessageEnvelope,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let (reply_agent, reply_team) = resolve_reply_target(message, current_team)?;
    let reply_team_dir = runtime.team_dir(home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }
    let reply_team_config = runtime.load_team_config(&reply_team_dir)?;
    if !reply_team_config
        .members
        .iter()
        .any(|member| member.name == reply_agent.as_str())
    {
        return Err(AtmError::agent_not_found(&reply_agent, &reply_team));
    }
    Ok(ReplyTarget::new(reply_agent, reply_team))
}

fn persist_acknowledged_source_state<R: RetainedMailboxRuntime>(
    runtime: &R,
    team: &TeamName,
    actor: &AgentName,
    message_key: &boundary::MessageKey,
    source_envelope: &MessageEnvelope,
    ack_timestamp: IsoTimestamp,
) -> Result<(), AtmError> {
    runtime.persist_message_state(boundary::MailMessageState {
        team: team.clone(),
        agent: actor.clone(),
        actor: actor.clone(),
        message_key: message_key.clone(),
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(ack_timestamp),
        expires_at: source_envelope.expires_at,
        deleted_at: None,
        updated_at: Some(ack_timestamp),
    })?;
    Ok(())
}

fn append_reply_to_target<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    reply_target: &ReplyTarget,
    reply_message: &MessageEnvelope,
) -> Result<(), AtmError> {
    let reply_inbox_path = runtime.inbox_path(home_dir, &reply_target.team, &reply_target.agent)?;
    append_mailbox_message_and_seed_workflow(
        runtime,
        home_dir,
        &reply_target.team,
        &reply_target.agent,
        &reply_inbox_path,
        reply_message,
        false,
    )
}

fn populate_ack_warnings<R: RetainedServiceRuntime>(
    runtime: &R,
    outcome: &mut AckOutcome,
    config: Option<&crate::config::AtmConfig>,
    actor: &AgentName,
    team: &TeamName,
    reply_message_id: AtmMessageId,
    reply_target: &ReplyTarget,
) {
    let hook_reply_recipient = ResolvedRecipient {
        agent: reply_target.agent.clone(),
        team: reply_target.team.clone(),
    };
    let mut hook_warnings = Vec::new();
    runtime.maybe_run_post_send_hook(
        &mut hook_warnings,
        config,
        PostSendHookContext {
            sender: actor,
            sender_team: Some(team),
            recipient: &hook_reply_recipient,
            recipient_pane_id: None,
            message_id: reply_message_id,
            requires_ack: false,
            is_ack: true,
            task_id: outcome.task_id.as_ref(),
        },
    );
    outcome.warnings = hook_warnings
        .into_iter()
        .map(|warning| warning.render())
        .collect();
}

fn resolve_reply_target(
    message: &MessageEnvelope,
    current_team: &TeamName,
) -> Result<(AgentName, TeamName), AtmError> {
    let identity = canonical_sender_identity(message);
    let team = message
        .source_team
        .clone()
        .or_else(|| Some(current_team.clone()))
        .ok_or_else(AtmError::team_unavailable)?;
    Ok((identity, team))
}

fn canonical_sender_identity(message: &MessageEnvelope) -> AgentName {
    crate::threading::canonical_sender_identity(message)
}

#[cfg(test)]
mod tests {
    use super::{canonical_sender_identity, resolve_reply_target};
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::MessageEnvelope;
    use crate::test_support::TEST_TEAM;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    fn message_with_from(from: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn canonical_sender_identity_uses_from_field() {
        let message = message_with_from(ROLE_TEAM_LEAD);
        assert_eq!(canonical_sender_identity(&message).as_str(), ROLE_TEAM_LEAD);
    }

    #[test]
    fn resolve_reply_target_uses_from_field() {
        let mut message = message_with_from(ROLE_TEAM_LEAD);
        message.source_team = Some(TEST_TEAM.parse::<TeamName>().expect("team"));

        let target = resolve_reply_target(&message, &TeamName::from_validated(TEST_TEAM))
            .expect("reply target");
        assert_eq!(
            target,
            (
                ROLE_TEAM_LEAD.parse().expect("agent"),
                TEST_TEAM.parse().expect("team"),
            )
        );
    }
}
