use std::path::PathBuf;

use serde::{Serialize, Serializer};
use serde_json::Map;
use tracing::trace;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorCode, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::inbox_export;
use crate::inbox_ingress;
use crate::mail_store::{MailStore, MessageSourceKind, StoredMessageRecord};
use crate::mailbox;
use crate::mailbox::source::{SourceFile, SourcedMessage};
use crate::mailbox::surface::dedupe_legacy_message_id_surface;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};
use crate::send::{
    PostSendHookContext, ResolvedRecipient, input, maybe_run_post_send_hook, summary,
};
use crate::store::{MessageKey, StoreDuplicateIdentity, StoreError};
use crate::task_store::TaskStore;
use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<AgentName>,
    pub team_override: Option<TeamName>,
    pub message_id: AckMessageId,
    pub reply_body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckMessageId {
    Legacy(LegacyMessageId),
    Atm(AtmMessageId),
}

impl std::fmt::Display for AckMessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy(value) => write!(f, "{value}"),
            Self::Atm(value) => write!(f, "{value}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AckCommitCommand<'a> {
    pub source_message_key: &'a MessageKey,
    pub reply_message: &'a StoredMessageRecord,
    pub acknowledged_at: IsoTimestamp,
    pub reply_team: &'a TeamName,
    pub reply_agent: &'a AgentName,
}

#[derive(Debug, Clone)]
pub struct AckCommitOutcome {
    pub acknowledged_task_ids: Vec<TaskId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckCommitRejection {
    AlreadyAcknowledged,
    NotPending,
}

#[derive(Debug, Clone)]
pub enum AckCommitResult {
    Committed(AckCommitOutcome),
    DuplicateReply(StoreDuplicateIdentity),
    Rejected(AckCommitRejection),
}

pub trait AckStore: MailStore + TaskStore {
    fn commit_ack_reply(
        &self,
        command: &AckCommitCommand<'_>,
    ) -> Result<AckCommitResult, StoreError>;
}

/// Summary of one successful acknowledgement and reply emission.
#[derive(Debug, Clone, Serialize)]
pub struct AckOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: String,
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

pub fn resolve_store_team(request: &AckRequest) -> Result<TeamName, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    config::resolve_team(
        request.team_override.as_ref().map(|team| team.as_str()),
        config.as_ref(),
    )
    .ok_or_else(AtmError::team_unavailable)
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
pub fn ack_mail<S>(
    request: AckRequest,
    store: &S,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError>
where
    S: AckStore,
{
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
    let source_files = mailbox::store::observe_source_files(&request.home_dir, &team, &actor)?;
    let source_message = find_source_message(&source_files, request.message_id, &actor, &team)?;
    reject_if_already_acknowledged(store, request.message_id)?;
    let _ = inbox_ingress::ingest_mailbox_state(
        &request.home_dir,
        &team,
        &actor,
        store,
        observability,
    )?;
    let source_legacy_message_id = source_message_legacy_message_id(&source_message.envelope)
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} has no legacy acknowledgement bridge identity",
                request.message_id
            ))
            .with_recovery(
                "Refresh the mailbox with `atm read` and retry after ATM reconstructs the legacy acknowledgement bridge from metadata.atm.messageId.",
            )
        })?;
    let stored_message = load_stored_message_for_request(store, request.message_id)?
        .ok_or_else(|| {
            AtmError::new_with_code(
                AtmErrorCode::AckInvalidState,
                AtmErrorKind::Validation,
                format!(
                    "message {} was not imported into SQLite acknowledgement state",
                    request.message_id
                ),
            )
            .with_recovery(
                "Refresh the mailbox with `atm read` and retry the acknowledgement after ATM imports the message into SQLite.",
            )
        })?;

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
    let mut reply_extra = Map::new();
    set_atm_message_id(&mut reply_extra, reply_atm_message_id);
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
        acknowledges_message_id: Some(source_legacy_message_id),
        task_id: None,
        extra: reply_extra,
    };
    let reply_stored_message = stored_reply_message(
        &reply_message,
        &actor,
        &team,
        &reply_agent,
        &reply_team,
        reply_message_id,
        reply_atm_message_id,
    )?;
    let commit_outcome = match store
        .commit_ack_reply(&AckCommitCommand {
            source_message_key: &stored_message.message_key,
            reply_message: &reply_stored_message,
            acknowledged_at: ack_timestamp,
            reply_team: &reply_team,
            reply_agent: &reply_agent,
        })
        .map_err(|error| map_store_error("failed to persist acknowledgement transition", error))?
    {
        AckCommitResult::Committed(outcome) => outcome,
        AckCommitResult::DuplicateReply(identity) => {
            return Err(duplicate_ack_reply_error(identity));
        }
        AckCommitResult::Rejected(rejection) => {
            return Err(ack_invalid_state_error(request.message_id, rejection));
        }
    };

    let source_task_id = commit_outcome.acknowledged_task_ids.first().cloned();
    let _ = observability.emit(CommandEvent {
        command: "ack",
        action: "commit",
        outcome: "ok",
        team: team.clone(),
        agent: actor.clone(),
        sender: actor.to_string(),
        message_id: Some(source_legacy_message_id),
        requires_ack: false,
        dry_run: false,
        task_id: source_task_id.clone(),
        error_code: None,
        error_message: None,
    });
    for task_id in &commit_outcome.acknowledged_task_ids {
        let _ = observability.emit(CommandEvent {
            command: "ack",
            action: "task_transition",
            outcome: "ok",
            team: team.clone(),
            agent: actor.clone(),
            sender: actor.to_string(),
            message_id: Some(source_legacy_message_id),
            requires_ack: false,
            dry_run: false,
            task_id: Some(task_id.clone()),
            error_code: None,
            error_message: None,
        });
    }

    let mut warnings = Vec::new();
    let export_succeeded = match inbox_export::export_message(
        &request.home_dir,
        &reply_team,
        &reply_agent,
        &reply_message,
        observability,
        inbox_export::ExportEventContext {
            command: "ack",
            sender: actor.to_string(),
            message_id: Some(source_legacy_message_id),
            requires_ack: false,
            task_id: source_task_id.clone(),
        },
    ) {
        Ok(()) => {
            let _ = observability.emit(CommandEvent {
                command: "ack",
                action: "export",
                outcome: "ok",
                team: team.clone(),
                agent: actor.clone(),
                sender: actor.to_string(),
                message_id: Some(source_legacy_message_id),
                requires_ack: false,
                dry_run: false,
                task_id: source_task_id.clone(),
                error_code: None,
                error_message: None,
            });
            true
        }
        Err(error) => {
            warnings.push(format!(
                "warning: acknowledgement reply export failed after SQLite commit: {}",
                error.message
            ));
            false
        }
    };

    let hook_reply_agent = reply_agent.clone();
    let hook_reply_team = reply_team.clone();
    let mut outcome = AckOutcome {
        action: "ack",
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id.to_string(),
        task_id: source_task_id.clone(),
        reply_target: ReplyTarget::new(reply_agent, reply_team),
        reply_message_id,
        reply_text: reply_text.clone(),
        warnings,
    };

    if export_succeeded {
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
                recipient_pane_id: None,
            },
        );
    }

    let _ = observability.emit(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor.to_string(),
        message_id: Some(source_legacy_message_id),
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
    if let Some(identity) = canonical_sender_identity(message) {
        let team = message
            .source_team
            .clone()
            .or_else(|| Some(current_team.clone()))
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

fn merged_surface(source_files: &[SourceFile]) -> Vec<SourcedMessage> {
    source_files
        .iter()
        .flat_map(|source| {
            source
                .messages
                .iter()
                .cloned()
                .enumerate()
                .map(|(source_index, envelope)| SourcedMessage {
                    envelope,
                    source_path: source.path.clone(),
                    source_index: source_index.into(),
                })
        })
        .collect()
}

fn find_source_message(
    source_files: &[SourceFile],
    message_id: AckMessageId,
    actor: &str,
    team: &str,
) -> Result<SourcedMessage, AtmError> {
    dedupe_legacy_message_id_surface(
        merged_surface(source_files),
        |message: &SourcedMessage| message.envelope.message_id,
        |message: &SourcedMessage| message.envelope.timestamp,
    )
    .into_iter()
    .filter_map(|message| {
        if message.envelope.message_id.is_none() && message.envelope.atm_message_id().is_none() {
            trace!(
                source_path = %message.source_path.display(),
                source_index = usize::from(message.source_index),
                "skipping source message without message identity during ack lookup"
            );
            return None;
        }
        Some(message)
    })
    .find(|message| message_matches_request_id(&message.envelope, message_id))
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

fn source_message_legacy_message_id(message: &MessageEnvelope) -> Option<LegacyMessageId> {
    message.message_id.or_else(|| {
        message
            .atm_message_id()
            .map(LegacyMessageId::from_atm_message_id)
    })
}

fn message_matches_request_id(message: &MessageEnvelope, request_id: AckMessageId) -> bool {
    match request_id {
        AckMessageId::Legacy(message_id) => {
            if message.message_id == Some(message_id) {
                return true;
            }
            message
                .atm_message_id()
                .is_some_and(|candidate| candidate == message_id.into_atm_message_id())
        }
        AckMessageId::Atm(message_id) => {
            if message.atm_message_id() == Some(message_id) {
                return true;
            }
            message.message_id.is_some_and(|candidate| {
                candidate == LegacyMessageId::from_atm_message_id(message_id)
            })
        }
    }
}

fn load_stored_message_for_request<S>(
    store: &S,
    message_id: AckMessageId,
) -> Result<Option<StoredMessageRecord>, AtmError>
where
    S: MailStore,
{
    match message_id {
        AckMessageId::Legacy(legacy_id) => {
            let stored = store
                .load_message_by_legacy_id(&legacy_id)
                .map_err(|error| {
                    map_store_error("failed to load acknowledged message from store", error)
                })?;
            if stored.is_some() {
                return Ok(stored);
            }
            store
                .load_message_by_atm_id(&legacy_id.into_atm_message_id())
                .map_err(|error| {
                    map_store_error("failed to load acknowledged message from store", error)
                })
        }
        AckMessageId::Atm(atm_id) => {
            let stored = store.load_message_by_atm_id(&atm_id).map_err(|error| {
                map_store_error("failed to load acknowledged message from store", error)
            })?;
            if stored.is_some() {
                return Ok(stored);
            }
            store
                .load_message_by_legacy_id(&LegacyMessageId::from_atm_message_id(atm_id))
                .map_err(|error| {
                    map_store_error("failed to load acknowledged message from store", error)
                })
        }
    }
}

fn reject_if_already_acknowledged<S>(store: &S, message_id: AckMessageId) -> Result<(), AtmError>
where
    S: MailStore,
{
    let Some(stored_message) = load_stored_message_for_request(store, message_id)? else {
        return Ok(());
    };
    let ack_state = store
        .load_ack_state(&stored_message.message_key)
        .map_err(|error| map_store_error("failed to load ack state", error))?;
    if ack_state
        .as_ref()
        .is_some_and(|state| state.acknowledged_at.is_some())
    {
        return Err(ack_invalid_state_error(
            message_id,
            AckCommitRejection::AlreadyAcknowledged,
        ));
    }
    Ok(())
}

fn stored_reply_message(
    reply_message: &MessageEnvelope,
    actor: &AgentName,
    source_team: &TeamName,
    reply_agent: &AgentName,
    reply_team: &TeamName,
    reply_message_id: LegacyMessageId,
    reply_atm_message_id: AtmMessageId,
) -> Result<StoredMessageRecord, AtmError> {
    let raw_metadata_json = reply_message
        .extra
        .get("metadata")
        .map(serde_json::to_string)
        .transpose()
        .map_err(|source| {
            AtmError::new(
                AtmErrorKind::Serialization,
                format!(
                    "failed to encode ATM metadata for acknowledgement reply to {}",
                    reply_agent
                ),
            )
            .with_source(source)
        })?;

    Ok(StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(reply_atm_message_id),
        team_name: reply_team.clone(),
        recipient_agent: reply_agent.clone(),
        sender_display: actor.to_string(),
        sender_canonical: Some(actor.clone()),
        sender_team: Some(source_team.clone()),
        body: reply_message.text.clone(),
        summary: reply_message.summary.clone(),
        created_at: reply_message.timestamp,
        source_kind: MessageSourceKind::Atm,
        legacy_message_id: Some(reply_message_id),
        atm_message_id: Some(reply_atm_message_id),
        raw_metadata_json,
    })
}

fn duplicate_ack_reply_error(identity: StoreDuplicateIdentity) -> AtmError {
    AtmError::new_with_code(
        AtmErrorCode::StoreConstraintViolation,
        AtmErrorKind::MailboxWrite,
        format!("generated duplicate acknowledgement reply identity: {identity:?}"),
    )
    .with_recovery(
        "Retry the acknowledgement once. If the duplicate persists, inspect the SQLite reply row identities before acknowledging again.",
    )
}

fn ack_invalid_state_error(message_id: AckMessageId, rejection: AckCommitRejection) -> AtmError {
    match rejection {
        AckCommitRejection::AlreadyAcknowledged => AtmError::new_with_code(
            AtmErrorCode::AckInvalidState,
            AtmErrorKind::Validation,
            format!("message {message_id} is already acknowledged"),
        )
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        ),
        AckCommitRejection::NotPending => AtmError::new_with_code(
            AtmErrorCode::AckInvalidState,
            AtmErrorKind::Validation,
            format!(
                "message {message_id} is not in the SQLite-authoritative (read, pending_ack) state"
            ),
        )
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        ),
    }
}

fn map_store_error(context: &str, error: StoreError) -> AtmError {
    let mut atm_error = AtmError::new_with_code(
        error.code,
        AtmErrorKind::MailboxWrite,
        format!("{context}: {}", error.message),
    );
    if let Some(recovery) = error.recovery.as_ref() {
        atm_error = atm_error.with_recovery(recovery.clone());
    }
    atm_error.with_source(error)
}

fn set_atm_message_id(extra: &mut Map<String, serde_json::Value>, message_id: AtmMessageId) {
    let metadata = extra
        .entry("metadata".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = serde_json::Value::Object(Map::new());
    }
    let Some(metadata) = metadata.as_object_mut() else {
        return;
    };
    let atm = metadata
        .entry("atm".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !atm.is_object() {
        *atm = serde_json::Value::Object(Map::new());
    }
    let Some(atm) = atm.as_object_mut() else {
        return;
    };
    atm.insert(
        "messageId".to_string(),
        serde_json::Value::String(message_id.to_string()),
    );
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

        let target = resolve_reply_target(&message, &"atm-dev".parse::<TeamName>().expect("team"))
            .expect("reply target");
        assert_eq!(
            target,
            (
                "team-lead".parse().expect("agent"),
                "atm-dev".parse().expect("team"),
            )
        );
    }
}
