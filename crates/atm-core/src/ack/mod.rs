use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};
use serde_json::Map;
use tracing::trace;

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::mailbox::source::{SourceFile, SourcedMessage};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};
use crate::send::{
    PostSendHookContext, ResolvedRecipient, input, maybe_run_post_send_hook, summary,
};
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};
use crate::workflow;

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub team_override: Option<TeamName>,
    pub message_id: LegacyMessageId,
    pub reply_body: String,
}

/// Summary of one successful acknowledgement and reply emission.
#[derive(Debug, Clone, Serialize)]
pub struct AckOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: LegacyMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_target: ReplyTarget,
    pub reply_message_id: LegacyMessageId,
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
    let config = config::load_config(&request.current_dir)?;
    let actor =
        identity::resolve_actor_identity(request.actor_override.as_deref(), config.as_ref())?;
    let team = config::resolve_team(request.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = home::team_dir_from_home(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    let team_config = config::load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == actor.as_str())
    {
        return Err(AtmError::agent_not_found(&actor, &team));
    }

    let source_workflow_path =
        home::workflow_state_path_from_home(&request.home_dir, &team, &actor)?;
    let source_workflow_state = workflow::load_workflow_state(&request.home_dir, &team, &actor)?;
    let source_files = mailbox::store::observe_source_files(&request.home_dir, &team, &actor)?;
    // Ack intentionally does not apply read-surface idle-notification dedup.
    // It must preserve the raw merged surface after legacy message_id
    // canonicalization so acknowledgement lookup does not depend on read-only
    // inbox clutter policy.
    let source_message = find_source_message(
        &source_files,
        &source_workflow_state,
        request.message_id,
        &actor,
        &team,
    )?;

    match (
        state::derive_read_state(&source_message.envelope),
        state::derive_ack_state(&source_message.envelope),
    ) {
        (crate::types::ReadState::Read, crate::types::AckState::PendingAck) => {}
        (_, crate::types::AckState::Acknowledged) => {
            return Err(AtmError::validation(format!(
                "message {} is already acknowledged",
                request.message_id
            ))
            .with_recovery(
                "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
            ));
        }
        _ => {
            return Err(AtmError::validation(format!(
                "message {} is not in the (read, pending_ack) state",
                request.message_id
            ))
            .with_recovery(
                "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
            ));
        }
    }

    let (reply_agent, reply_team) = resolve_reply_target(&source_message.envelope, &team)?;
    let reply_team_dir = home::team_dir_from_home(&request.home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }

    let reply_team_config = config::load_team_config(&reply_team_dir)?;
    if !reply_team_config
        .members
        .iter()
        .any(|member| member.name == reply_agent.as_str())
    {
        return Err(AtmError::agent_not_found(&reply_agent, &reply_team));
    }

    let (reply_atm_message_id, ack_timestamp) = AtmMessageId::new_with_timestamp();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_message_id = LegacyMessageId::new();
    let source_task_id = source_message.envelope.task_id.clone();
    let mut reply_extra = Map::new();
    workflow::set_atm_message_id(&mut reply_extra, reply_atm_message_id);
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
        task_id: None,
        extra: reply_extra,
    };

    let reply_inbox_path =
        home::inbox_path_from_home(&request.home_dir, &reply_team, &reply_agent)?;
    let reply_workflow_path =
        home::workflow_state_path_from_home(&request.home_dir, &reply_team, &reply_agent)?;
    let reply_targets_source_mailbox =
        reply_team.as_str() == team.as_str() && reply_agent.as_str() == actor.as_str();
    // Ack intentionally does not hold a subset lock and then upgrade it.
    // Resolve the reply target from an unlocked preflight, then let the shared
    // commit helper acquire the final sorted superset, reload, and re-validate
    // before mutating either inbox.
    mailbox::store::with_locked_source_files(
        &request.home_dir,
        &team,
        &actor,
        [
            reply_inbox_path.clone(),
            source_workflow_path,
            reply_workflow_path,
        ],
        mailbox::lock::default_lock_timeout(),
        |_source_paths, source_files| {
            let mut source_workflow_state =
                workflow::load_workflow_state(&request.home_dir, &team, &actor)?;
            let mut reply_workflow_state = (!reply_targets_source_mailbox)
                .then(|| {
                    workflow::load_workflow_state(&request.home_dir, &reply_team, &reply_agent)
                })
                .transpose()?;
            let source_message = find_source_message(
                source_files,
                &source_workflow_state,
                request.message_id,
                &actor,
                &team,
            )?;
            match (
                state::derive_read_state(&source_message.envelope),
                state::derive_ack_state(&source_message.envelope),
            ) {
                (crate::types::ReadState::Read, crate::types::AckState::PendingAck) => {}
                _ => {
                    return Err(AtmError::validation(format!(
                        "message {} is not in the (read, pending_ack) state",
                        request.message_id
                    ))
                    .with_recovery(
                        "Refresh the mailbox with `atm read` and retry the acknowledgement if the message is still pending acknowledgement.",
                    ));
                }
            }
            let mailbox_changed = update_source_message(
                source_files,
                &mut source_workflow_state,
                &source_message,
                ack_timestamp,
            )?;
            append_reply_message(source_files, &reply_inbox_path, reply_message.clone())?;
            mailbox::store::commit_source_files(source_files)?;
            if reply_targets_source_mailbox {
                workflow::remember_initial_state(&mut source_workflow_state, &reply_message);
                workflow::save_workflow_state(
                    &request.home_dir,
                    &team,
                    &actor,
                    &source_workflow_state,
                )?;
            } else {
                workflow::save_workflow_state(
                    &request.home_dir,
                    &team,
                    &actor,
                    &source_workflow_state,
                )?;
            }
            if let Some(reply_workflow_state) = reply_workflow_state.as_mut() {
                workflow::remember_initial_state(reply_workflow_state, &reply_message);
                workflow::save_workflow_state(
                    &request.home_dir,
                    &reply_team,
                    &reply_agent,
                    reply_workflow_state,
                )?;
            }
            Ok(mailbox_changed)
        },
    )?;

    let hook_reply_agent = reply_agent.clone();
    let hook_reply_team = reply_team.clone();
    let mut outcome = AckOutcome {
        action: "ack",
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        task_id: source_task_id.clone(),
        reply_target: ReplyTarget::new(reply_agent, reply_team),
        reply_message_id,
        reply_text: reply_text.clone(),
        warnings: Vec::new(),
    };

    let hook_reply_recipient = ResolvedRecipient {
        agent: hook_reply_agent,
        team: hook_reply_team,
    };
    maybe_run_post_send_hook(
        &mut outcome.warnings,
        config.as_ref(),
        PostSendHookContext {
            sender: &actor,
            sender_team: Some(&team),
            recipient: &hook_reply_recipient,
            message_id: reply_message_id,
            requires_ack: false,
            is_ack: true,
            task_id: outcome.task_id.as_ref(),
        },
    );

    let _ = observability.emit(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor.to_string(),
        message_id: Some(request.message_id),
        requires_ack: false,
        dry_run: false,
        task_id: source_task_id,
        error_code: None,
        error_message: None,
    });

    Ok(outcome)
}

fn resolve_reply_target(
    message: &MessageEnvelope,
    current_team: &str,
) -> Result<(AgentName, TeamName), AtmError> {
    if let Some(identity) = canonical_sender_identity(message) {
        let team = message
            .source_team
            .clone()
            .or_else(|| Some(current_team.parse().expect("validated team")))
            .ok_or_else(AtmError::team_unavailable)?;
        return Ok((identity, team));
    }

    let parsed: AgentAddress = if message.from.contains('@') {
        message.from.as_str().parse()?
    } else {
        AgentAddress {
            agent: message.from.to_string(),
            team: message
                .source_team
                .clone()
                .map(Into::into)
                .or_else(|| Some(current_team.to_string())),
        }
    };

    let team = parsed.team.ok_or_else(AtmError::team_unavailable)?;
    Ok((
        AgentName::from_validated(parsed.agent),
        TeamName::from_validated(team),
    ))
}

fn canonical_sender_identity(message: &MessageEnvelope) -> Option<AgentName> {
    message
        .extra
        .get("metadata")
        .and_then(serde_json::Value::as_object)
        .and_then(|metadata| metadata.get("atm"))
        .and_then(serde_json::Value::as_object)
        .and_then(|atm| atm.get("fromIdentity"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn merged_surface(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
) -> Vec<SourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, envelope)| SourcedMessage {
                    envelope: workflow::project_envelope(&envelope, workflow_state),
                    source_path: source.path.clone(),
                    source_index: source_index.into(),
                })
        })
        .collect()
}

fn find_source_message(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
    message_id: LegacyMessageId,
    actor: &str,
    team: &str,
) -> Result<SourcedMessage, AtmError> {
    dedupe_legacy_message_id_surface(
        merged_surface(source_files, workflow_state),
        |message: &SourcedMessage| message.envelope.message_id,
        |message: &SourcedMessage| message.envelope.timestamp,
    )
    .into_iter()
    .filter_map(|message| match message.envelope.message_id {
        Some(_) => Some(message),
        None => {
            trace!(
                source_path = %message.source_path.display(),
                source_index = usize::from(message.source_index),
                "skipping source message without message_id during ack lookup"
            );
            None
        }
    })
    .find(|message| message.envelope.message_id == Some(message_id))
    .ok_or_else(|| {
        AtmError::validation(format!(
            "message {} was not found in {}@{}",
            message_id, actor, team
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still present in the pending-ack surface.",
        )
    })
}

fn update_source_message(
    source_files: &mut [SourceFile],
    workflow_state: &mut workflow::WorkflowStateFile,
    source_message: &SourcedMessage,
    acknowledged_at: IsoTimestamp,
) -> Result<bool, AtmError> {
    let transitioned = state::StoredMessage::<
        crate::types::ReadReadState,
        crate::types::PendingAckState,
    >::read_pending_ack(source_message.envelope.clone())
    .acknowledge(acknowledged_at)
    .envelope;

    if workflow::apply_projected_state(workflow_state, &source_message.envelope, &transitioned) {
        return Ok(false);
    }

    let source_file = source_files
        .iter_mut()
        .find(|source| source.path == source_message.source_path)
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "source inbox disappeared during acknowledgement: {}",
                source_message.source_path.display()
            ))
        })?;

    let stored = source_file
        .messages
        .get_mut(source_message.source_index.get())
        .ok_or_else(|| {
            AtmError::mailbox_write(format!(
                "source message index {} disappeared during acknowledgement",
                usize::from(source_message.source_index)
            ))
        })?;
    *stored = transitioned;
    Ok(true)
}

fn append_reply_message(
    source_files: &mut Vec<SourceFile>,
    reply_inbox_path: &Path,
    reply_message: MessageEnvelope,
) -> Result<(), AtmError> {
    if let Some(source_file) = source_files
        .iter_mut()
        .find(|source| source.path == reply_inbox_path)
    {
        source_file.messages.push(reply_message);
        return Ok(());
    }

    source_files.push(SourceFile {
        path: reply_inbox_path.to_path_buf(),
        messages: mailbox::read_messages(reply_inbox_path)?,
    });
    source_files
        .last_mut()
        .expect("Vec::push is infallible — last_mut always returns Some after push")
        .messages
        .push(reply_message);
    source_files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{canonical_sender_identity, resolve_reply_target};
    use crate::schema::MessageEnvelope;
    use crate::types::{AgentName, IsoTimestamp, TeamName};

    fn message_with_from(from: &str) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("atm-dev".parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn canonical_sender_identity_reads_metadata_override() {
        let mut message = message_with_from("lead");
        message.extra.insert(
            "metadata".to_string(),
            json!({"atm": {"fromIdentity": "team-lead"}}),
        );

        assert_eq!(
            canonical_sender_identity(&message).as_deref(),
            Some("team-lead")
        );
    }

    #[test]
    fn resolve_reply_target_prefers_canonical_sender_identity_metadata() {
        let mut message = message_with_from("lead");
        message.source_team = Some("atm-dev".parse::<TeamName>().expect("team"));
        message.extra.insert(
            "metadata".to_string(),
            json!({"atm": {"fromIdentity": "team-lead"}}),
        );

        let target = resolve_reply_target(&message, "atm-dev").expect("reply target");
        assert_eq!(
            target,
            (
                "team-lead".parse().expect("agent"),
                "atm-dev".parse().expect("team"),
            )
        );
    }
}
