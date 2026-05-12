use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Map;
use tracing::trace;

use crate::config;
use crate::error::AtmError;
use crate::identity;
use crate::mailbox::source::{SourceFile, SourcedMessage};
use crate::mailbox::surface::dedupe_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{AtmMessageId, MessageEnvelope};
use crate::send::{PostSendHookContext, ResolvedRecipient, input, summary};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::RetainedMailboxRuntime;
use crate::threading::ThreadIndex;
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use crate::workflow;

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
    let runtime = LocalServiceRuntime::default();
    ack_mail_with_runtime(request, observability, &runtime)
}

fn ack_mail_with_runtime<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
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

    let source_workflow_path = runtime.workflow_state_path(&request.home_dir, &team, &actor)?;
    let source_workflow_state = runtime.load_workflow_state(&request.home_dir, &team, &actor)?;
    let source_files = runtime.observe_source_files(&request.home_dir, &team, &actor)?;
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
    reject_non_terminal_ack(&source_files, &source_workflow_state, request.message_id)?;

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
    let reply_team_dir = runtime.team_dir(&request.home_dir, &reply_team)?;
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

    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_message_id = AtmMessageId::new();
    let source_task_id = source_message.envelope.task_id.clone();
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
        stale_at: None,
        task_id: None,
        extra: Map::new(),
    };

    let reply_inbox_path = runtime.inbox_path(&request.home_dir, &reply_team, &reply_agent)?;
    let reply_workflow_path =
        runtime.workflow_state_path(&request.home_dir, &reply_team, &reply_agent)?;
    let reply_targets_source_mailbox =
        reply_team.as_str() == team.as_str() && reply_agent.as_str() == actor.as_str();
    // Ack intentionally does not hold a subset lock and then upgrade it.
    // Resolve the reply target from an unlocked preflight, then let the shared
    // commit helper acquire the final sorted superset, reload, and re-validate
    // before mutating either inbox.
    runtime.with_locked_source_files(
        &request.home_dir,
        &team,
        &actor,
        [
            reply_inbox_path.clone(),
            source_workflow_path,
            reply_workflow_path,
        ],
        runtime.mailbox_timeout_policy().workflow_lock_timeout,
        |_source_paths, source_files| {
            let mut source_workflow_state =
                runtime.load_workflow_state(&request.home_dir, &team, &actor)?;
            let mut reply_workflow_state = (!reply_targets_source_mailbox)
                .then(|| runtime.load_workflow_state(&request.home_dir, &reply_team, &reply_agent))
                .transpose()?;
            let source_message = find_source_message(
                source_files,
                &source_workflow_state,
                request.message_id,
                &actor,
                &team,
            )?;
            reject_non_terminal_ack(source_files, &source_workflow_state, request.message_id)?;
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
            append_reply_message(runtime, source_files, &reply_inbox_path, reply_message.clone())?;
            runtime.commit_source_files(source_files)?;
            if reply_targets_source_mailbox {
                workflow::remember_initial_state(&mut source_workflow_state, &reply_message);
                runtime.save_workflow_state(
                    &request.home_dir,
                    &team,
                    &actor,
                    &source_workflow_state,
                )?;
            } else {
                runtime.save_workflow_state(
                    &request.home_dir,
                    &team,
                    &actor,
                    &source_workflow_state,
                )?;
            }
            if let Some(reply_workflow_state) = reply_workflow_state.as_mut() {
                workflow::remember_initial_state(reply_workflow_state, &reply_message);
                runtime.save_workflow_state(
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
        action: CommandAction::Ack,
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
    let mut hook_warnings = Vec::new();
    runtime.maybe_run_post_send_hook(
        &mut hook_warnings,
        config.as_ref(),
        PostSendHookContext {
            sender: &actor,
            sender_team: Some(&team),
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

    let _ = observability.emit(CommandEvent {
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
    });

    Ok(outcome)
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

fn reject_non_terminal_ack(
    source_files: &[SourceFile],
    workflow_state: &workflow::WorkflowStateFile,
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    let envelopes = merged_surface(source_files, workflow_state)
        .into_iter()
        .map(|message| message.envelope)
        .collect::<Vec<_>>();
    let index = ThreadIndex::new(&envelopes);
    let Some(terminal_id) = index.terminal_id(message_id) else {
        return Ok(());
    };
    if terminal_id == message_id {
        return Ok(());
    }
    Err(AtmError::validation(format!(
        "message {} has been updated; acknowledge the current terminal message {} instead",
        message_id, terminal_id
    ))
    .with_recovery(
        "Refresh the mailbox with `atm read` and acknowledge the latest message in the thread instead of an older parent message.",
    ))
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
    message_id: AtmMessageId,
    actor: &AgentName,
    team: &TeamName,
) -> Result<SourcedMessage, AtmError> {
    dedupe_message_id_surface(
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
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
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
        messages: runtime.read_messages(reply_inbox_path)?,
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
    use std::path::PathBuf;

    use super::{
        canonical_sender_identity, find_source_message, reject_non_terminal_ack,
        resolve_reply_target,
    };
    use crate::mailbox::source::SourceFile;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use crate::workflow;

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
            stale_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    fn thread_message(
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: ROLE_TEAM_LEAD.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: true,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id,
            thread_mode,
            stale_at: None,
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

    #[test]
    fn reject_non_terminal_ack_requires_latest_thread_message() {
        let root_id = AtmMessageId::new();
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![
                thread_message(root_id, None, None),
                thread_message(
                    AtmMessageId::new(),
                    Some(root_id),
                    Some(ThreadMode::Supersede),
                ),
            ],
        }];

        let error = reject_non_terminal_ack(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            root_id,
        )
        .expect_err("stale parent ack");

        assert!(error.message.contains("current terminal message"));
    }

    #[test]
    fn find_source_message_accepts_uuid_wire_form_for_ack_lookup() {
        let message_id: AtmMessageId = "01KRFK5QTF2R6NRS3Q0F8Z9K0S"
            .parse()
            .expect("atm message id");
        let source_files = vec![SourceFile {
            path: PathBuf::from("recipient.json"),
            messages: vec![thread_message(message_id, None, None)],
        }];
        let parsed_from_uuid: AtmMessageId = message_id
            .into_uuid_wire()
            .to_string()
            .parse()
            .expect("uuid wire parse");

        let found = find_source_message(
            &source_files,
            &workflow::WorkflowStateFile::default(),
            parsed_from_uuid,
            &AgentName::from_validated(TEST_SENDER),
            &TeamName::from_validated(TEST_TEAM),
        )
        .expect("source message");

        assert_eq!(found.envelope.message_id, Some(message_id));
    }
}
