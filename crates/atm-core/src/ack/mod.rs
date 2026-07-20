use std::path::PathBuf;

use crate::boundary;
use crate::boundary::PostSendHookEmitter;
use crate::error::AtmError;
use crate::observability::{action_name, outcome_label, CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{remote_host as message_remote_host, AtmMessageId, InboxMessage};
use crate::send::{
    input, send_mail_with_runtime, send_mail_with_runtime_and_post_send_emitter, summary,
    RemoteTargetHost, SendCommandOutcome, SendMessageSource, SendOutcome, SendRequest,
};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{default_runtime, RetainedMailboxRuntime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

/// Summary of one successful acknowledgement and reply handling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    pub reply_message_id: AtmMessageId,
    pub reply_target: ReplyTarget,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::send::WarningEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
    remote_host: Option<String>,
}

impl ReplyTarget {
    fn new(agent: AgentName, team: TeamName, remote_host: Option<String>) -> Self {
        Self {
            agent,
            team,
            remote_host,
        }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)?;
        if let Some(remote_host) = &self.remote_host {
            write!(f, ".{remote_host}")?;
        }
        Ok(())
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
        let (agent, team_and_host) = value
            .split_once('@')
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        let (team, remote_host) = match team_and_host.split_once('.') {
            Some((team, remote_host)) => (team, Some(remote_host.to_string())),
            None => (team_and_host, None),
        };
        Ok(Self::new(
            agent.parse().map_err(serde::de::Error::custom)?,
            team.parse().map_err(serde::de::Error::custom)?,
            remote_host,
        ))
    }
}

/// Acknowledge one previously read pending-ack message and emit the documented
/// reply disposition.
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
    let prepared = prepare_ack_reply(runtime, request)?;
    reject_remote_core_ack(&prepared)?;
    let outcome = send_mail_with_runtime(prepared.reply_request.clone(), observability, runtime)?;
    finalize_ack_after_send(runtime, observability, prepared, outcome)
}

pub fn ack_mail_with_runtime_and_post_send_emitter(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<AckOutcome, AtmError> {
    let prepared = prepare_ack_reply(runtime, request)?;
    reject_remote_core_ack(&prepared)?;
    let outcome = send_mail_with_runtime_and_post_send_emitter(
        prepared.reply_request.clone(),
        observability,
        runtime,
        post_send_emitter,
    )?;
    finalize_ack_after_send(runtime, observability, prepared, outcome)
}

pub fn prepare_ack_send_request(request: AckRequest) -> Result<SendRequest, AtmError> {
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_summary = summary::build_summary(&reply_text, None);
    SendRequest::new(
        request.home_dir,
        request.current_dir,
        actor.clone(),
        &format!("{actor}@{team}"),
        team,
        SendMessageSource::Inline(reply_text),
        Some(reply_summary),
        false,
        None,
        false,
    )
    .map(|mut send_request| {
        send_request.acknowledges_message_id = Some(request.message_id);
        send_request
    })
}

pub fn ack_request_from_send_request(request: SendRequest) -> Result<AckRequest, AtmError> {
    let message_id = request.acknowledges_message_id.ok_or_else(|| {
        AtmError::validation(
            "canonical send request is missing acknowledges_message_id for ack handling"
                .to_string(),
        )
        .with_recovery("Construct ack replies through the canonical ack preparation helper.")
    })?;
    let reply_body = match request.message_source {
        SendMessageSource::Inline(message) => message,
        SendMessageSource::File {
            message: Some(message),
            ..
        } => message,
        SendMessageSource::File { message: None, .. } => {
            return Err(AtmError::validation(
                "canonical ack send request must carry the reply body inline".to_string(),
            )
            .with_recovery(
                "Materialize the ack reply body before dispatching the canonical send request.",
            ));
        }
    };
    Ok(AckRequest {
        home_dir: request.home_dir,
        current_dir: request.current_dir,
        caller_identity: request.caller_identity,
        caller_team: request.caller_team,
        message_id,
        reply_body,
    })
}

#[derive(Clone)]
struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
}

/// Ack data retained only until the canonical outbound send confirms delivery.
pub struct PreparedAckReply {
    pub reply_request: SendRequest,
    actor: AgentName,
    team: TeamName,
    source_message_id: AtmMessageId,
    reply_target: ReplyTarget,
    reply_text: String,
    task_id: Option<TaskId>,
    source_message_key: boundary::MessageKey,
    source_expires_at: Option<IsoTimestamp>,
    ack_timestamp: IsoTimestamp,
}

fn load_ack_source<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    message_id: AtmMessageId,
) -> Result<LoadedAckSource, AtmError> {
    let metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, actor, None)?;
    let source_row = find_ack_source_row(&metadata_rows, message_id, actor, team)?;
    ensure_ack_target_is_terminal(&metadata_rows, message_id)?;
    let source_record = load_ack_source_record(runtime, home_dir, team, actor, source_row)?;
    ensure_ack_is_pending(message_id, &source_record.envelope)?;
    Ok(LoadedAckSource {
        row: source_row.clone(),
    })
}

fn find_ack_source_row<'a>(
    metadata_rows: &'a [boundary::MailStoreMailboxMetadataRow],
    message_id: AtmMessageId,
    actor: &AgentName,
    team: &TeamName,
) -> Result<&'a boundary::MailStoreMailboxMetadataRow, AtmError> {
    metadata_rows
        .iter()
        .find(|row| row.message_id == Some(message_id))
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

fn ensure_ack_target_is_terminal(
    metadata_rows: &[boundary::MailStoreMailboxMetadataRow],
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    if metadata_rows
        .iter()
        .any(|row| row.parent_message_id == Some(message_id))
    {
        return Err(AtmError::validation(format!(
            "message {} has been updated; acknowledge the current terminal message instead",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and acknowledge the latest message in the thread instead of an older parent message.",
        ));
    }
    Ok(())
}

fn load_ack_source_record<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    source_row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<boundary::Message, AtmError> {
    runtime
        .load_message_record(home_dir, team, actor, &source_row.message_key)?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} metadata could not be reloaded from sqlite",
                source_row
                    .message_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string())
            ))
            .with_recovery(
                "Repair or remove the malformed sqlite mailbox row before retrying `atm ack`.",
            )
        })
}

fn ensure_ack_is_pending(message_id: AtmMessageId, source: &InboxMessage) -> Result<(), AtmError> {
    match state::derive_ack_state(source) {
        crate::types::AckState::PendingAck => Ok(()),
        crate::types::AckState::Acknowledged => Err(AtmError::validation(format!(
            "message {} is already acknowledged",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
        crate::types::AckState::NoAckRequired => Err(AtmError::validation(format!(
            "message {} is not pending acknowledgement",
            message_id
        ))
        .with_recovery(
            "Refresh the mailbox with `atm read` and choose a message that is still pending acknowledgement.",
        )),
    }
}

fn validate_reply_target<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    source_record: &boundary::Message,
    current_actor: &AgentName,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let (reply_agent, reply_team) = resolve_reply_target(&source_record.envelope, current_team);
    let remote_host = message_remote_host(&source_record.envelope).map(str::to_owned);
    if remote_host.is_none()
        && current_actor
            .as_str()
            .eq_ignore_ascii_case(reply_agent.as_str())
        && current_team
            .as_str()
            .eq_ignore_ascii_case(reply_team.as_str())
    {
        return Err(AtmError::validation(
            "local self-ack is not allowed without an explicit host target".to_string(),
        )
        .with_recovery(
            "Use an explicit host target such as `<agent>@<team>.localhost` or `<agent>@<team>.<self-ip>` for same-host proof traffic, or acknowledge a message from another sender.",
        ));
    }
    if remote_host.is_none() {
        ensure_roster_member_exists(
            runtime,
            &reply_team,
            &reply_agent,
            "Repair or reload the ATM roster before retrying the acknowledgement reply.",
        )?;
    }

    Ok(ReplyTarget::new(reply_agent, reply_team, remote_host))
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::agent_not_found(agent, team).with_recovery(recovery));
    }

    Ok(())
}

/// Prepares an acknowledgement as the canonical outbound [`SendRequest`].
///
/// The daemon dispatches this request through the same local-or-remote route
/// used by ordinary sends; source acknowledgement state remains unchanged
/// until [`finalize_ack_after_send`] receives a confirmed send outcome.
pub fn prepare_ack_reply(
    runtime: &LocalServiceRuntime,
    request: AckRequest,
) -> Result<PreparedAckReply, AtmError> {
    prepare_ack_reply_with_runtime(runtime, request)
}

fn prepare_ack_reply_with_runtime<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: AckRequest,
) -> Result<PreparedAckReply, AtmError> {
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    ensure_roster_member_exists(
        runtime,
        &team,
        &actor,
        "Repair or reload the ATM roster before retrying `atm ack`.",
    )?;
    let source = load_ack_source(
        runtime,
        &request.home_dir,
        &team,
        &actor,
        request.message_id,
    )?;
    let source_record =
        load_ack_source_record(runtime, &request.home_dir, &team, &actor, &source.row)?;
    let reply_target = validate_reply_target(runtime, &source_record, &actor, &team)?;
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_summary = summary::build_summary(&reply_text, None);
    let reply_request = SendRequest {
        home_dir: request.home_dir,
        current_dir: request.current_dir,
        caller_identity: actor.clone(),
        caller_team: team.clone(),
        to: format!("{}@{}", reply_target.agent, reply_target.team).parse()?,
        message_source: SendMessageSource::Inline(reply_text.clone()),
        summary_override: Some(reply_summary),
        requires_ack: false,
        task_id: source_record.envelope.task_id.clone(),
        parent_message_id: None,
        acknowledges_message_id: Some(request.message_id),
        thread_mode: None,
        expires_at: None,
        source_remote_host: None,
        remote_host: reply_target
            .remote_host
            .as_deref()
            .map(RemoteTargetHost::parse)
            .transpose()?,
        dry_run: false,
    };
    Ok(PreparedAckReply {
        reply_request,
        actor,
        team,
        source_message_id: request.message_id,
        reply_target,
        reply_text,
        task_id: source_record.envelope.task_id,
        source_message_key: source.row.message_key,
        source_expires_at: source_record.envelope.expires_at,
        ack_timestamp: IsoTimestamp::now(),
    })
}

/// Commits source acknowledgement state only after canonical send dispatch
/// reports a confirmed delivery.
pub fn finalize_ack_after_send(
    runtime: &LocalServiceRuntime,
    observability: &dyn ObservabilityPort,
    prepared: PreparedAckReply,
    outcome: SendOutcome,
) -> Result<AckOutcome, AtmError> {
    finalize_ack_after_send_with_runtime(runtime, observability, prepared, outcome)
}

fn finalize_ack_after_send_with_runtime<R: RetainedMailboxRuntime + ?Sized>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    prepared: PreparedAckReply,
    outcome: SendOutcome,
) -> Result<AckOutcome, AtmError> {
    if outcome.outcome != SendCommandOutcome::Sent {
        return Err(AtmError::daemon_unavailable(
            "ack reply delivery was not confirmed; the source message remains pending acknowledgement",
        )
        .with_recovery(
            "Restore the remote daemon connection and retry `atm ack`; ATM will not mark the source message acknowledged until the reply is delivered.",
        ));
    }
    runtime.persist_message_state(boundary::MailMessageState {
        team: prepared.team.clone(),
        agent: prepared.actor.clone(),
        actor: prepared.actor.clone(),
        message_key: prepared.source_message_key.clone(),
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(prepared.ack_timestamp),
        expires_at: prepared.source_expires_at,
        deleted_at: None,
        updated_at: Some(prepared.ack_timestamp),
    })?;
    record_ack_telemetry(
        observability,
        &prepared.actor,
        prepared.team.clone(),
        prepared.source_message_id,
        prepared.task_id.clone(),
    );
    Ok(AckOutcome {
        action: CommandAction::Ack,
        team: prepared.team,
        agent: prepared.actor,
        message_id: prepared.source_message_id,
        task_id: prepared.task_id,
        reply_message_id: outcome.message_id,
        reply_target: prepared.reply_target,
        reply_text: prepared.reply_text,
        warnings: outcome.warnings,
    })
}

fn reject_remote_core_ack(prepared: &PreparedAckReply) -> Result<(), AtmError> {
    if prepared.reply_request.remote_host.is_some() {
        return Err(AtmError::daemon_unavailable(
            "remote acknowledgement delivery requires daemon request dispatch",
        )
        .with_recovery(
            "Run `atm ack` through atm-daemon so the canonical outbound send handler can deliver the remote reply.",
        ));
    }
    Ok(())
}

fn record_ack_telemetry(
    observability: &dyn ObservabilityPort,
    actor: &AgentName,
    team: TeamName,
    message_id: AtmMessageId,
    task_id: Option<TaskId>,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "ack",
        action: action_name("ack"),
        outcome: outcome_label("ok"),
        team,
        agent: actor.clone(),
        sender: actor.clone(),
        message_id: Some(message_id),
        requires_ack: false,
        dry_run: false,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        tracing::warn!(
            %error,
            subsystem = "ack",
            outcome = "emit_failed",
            command = "ack",
            action = "ack",
            "failed to emit ack command event"
        );
    }
}

fn resolve_reply_target(message: &InboxMessage, current_team: &TeamName) -> (AgentName, TeamName) {
    let identity = canonical_sender_identity(message);
    let team = message
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    (identity, team)
}

fn canonical_sender_identity(message: &InboxMessage) -> AgentName {
    crate::threading::canonical_sender_identity(message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use super::{
        canonical_sender_identity, finalize_ack_after_send_with_runtime, resolve_reply_target,
        PreparedAckReply, ReplyTarget,
    };
    use crate::boundary::{MailMessageState, Message, MessageKey};
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::schema::{AckIntentFields, InboxMessage};
    use crate::send::{SendCommandOutcome, SendOutcome, SendRequest};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::types::{AgentName, CommandAction, IsoTimestamp, TeamName};

    #[derive(Default)]
    struct RecordingMailbox {
        states: Mutex<Vec<MailMessageState>>,
    }

    impl RetainedMailboxRuntime for RecordingMailbox {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<crate::boundary::MailStoreMailboxMetadataRow>, AtmError> {
            unreachable!("finalization only writes source acknowledgement state")
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &MessageKey,
        ) -> Result<Option<Message>, AtmError> {
            unreachable!("finalization only writes source acknowledgement state")
        }

        fn persist_message_record(&self, _record: Message) -> Result<(), AtmError> {
            unreachable!("finalization only writes source acknowledgement state")
        }

        fn persist_message_state(&self, state: MailMessageState) -> Result<(), AtmError> {
            self.states.lock().expect("states").push(state);
            Ok(())
        }
    }

    fn message(from: &str, team: &str) -> InboxMessage {
        let ack = AckIntentFields::not_required();
        InboxMessage {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(team.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: None,
            requires_ack: ack.requires_ack,
            pending_ack_at: ack.pending_ack_at,
            acknowledged_at: ack.acknowledged_at,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn reply_target_round_trips_remote_host() {
        let target = ReplyTarget::new(
            AgentName::from_validated("sender"),
            TeamName::from_validated("team"),
            Some("127.0.0.1".to_string()),
        );
        let encoded = serde_json::to_string(&target).expect("serialize target");
        let decoded: ReplyTarget = serde_json::from_str(&encoded).expect("deserialize target");
        assert_eq!(decoded, target);
    }

    #[test]
    fn reply_target_uses_message_sender_and_team() {
        let message = message("sender", "origin-team");
        assert_eq!(canonical_sender_identity(&message).as_str(), "sender");
        assert_eq!(
            resolve_reply_target(&message, &TeamName::from_validated("fallback")),
            (
                AgentName::from_validated("sender"),
                TeamName::from_validated("origin-team")
            ),
        );
    }

    fn prepared_ack() -> PreparedAckReply {
        let agent = AgentName::from_validated("recipient");
        let team = TeamName::from_validated("team");
        PreparedAckReply {
            reply_request: SendRequest::new(
                "/tmp".into(),
                "/tmp".into(),
                agent.clone(),
                "recipient@team",
                team.clone(),
                crate::send::SendMessageSource::Inline("ack".to_string()),
                None,
                false,
                None,
                false,
            )
            .expect("request"),
            actor: agent.clone(),
            team: team.clone(),
            source_message_id: crate::schema::AtmMessageId::new(),
            reply_target: ReplyTarget::new(
                AgentName::from_validated("sender"),
                team,
                Some("127.0.0.1".to_string()),
            ),
            reply_text: "ack".to_string(),
            task_id: None,
            source_message_key: MessageKey::new("atm:source").expect("message key"),
            source_expires_at: None,
            ack_timestamp: IsoTimestamp::now(),
        }
    }

    fn outcome(command: SendCommandOutcome) -> SendOutcome {
        SendOutcome {
            action: CommandAction::Send,
            team: TeamName::from_validated("team"),
            agent: AgentName::from_validated("sender"),
            sender: AgentName::from_validated("recipient"),
            outcome: command,
            message_id: crate::schema::AtmMessageId::new(),
            receipt_message_id: None,
            requires_ack: false,
            task_id: None,
            summary: None,
            message: None,
            warnings: Vec::new(),
            dry_run: false,
        }
    }

    #[test]
    fn deferred_ack_send_leaves_source_pending() {
        let runtime = RecordingMailbox::default();
        let error = finalize_ack_after_send_with_runtime(
            &runtime,
            &NullObservability,
            prepared_ack(),
            outcome(SendCommandOutcome::Deferred),
        )
        .expect_err("deferred send must not acknowledge source");

        assert!(error.is_daemon_unavailable());
        assert!(runtime.states.lock().expect("states").is_empty());
    }

    #[test]
    fn confirmed_ack_send_commits_source_acknowledgement() {
        let runtime = RecordingMailbox::default();
        let ack = finalize_ack_after_send_with_runtime(
            &runtime,
            &NullObservability,
            prepared_ack(),
            outcome(SendCommandOutcome::Sent),
        )
        .expect("confirmed send acknowledges source");

        let states = runtime.states.lock().expect("states");
        assert_eq!(states.len(), 1);
        assert!(states[0].acknowledged_at.is_some());
        assert!(states[0].pending_ack_at.is_none());
        assert_eq!(ack.action, CommandAction::Ack);
    }
}
