use std::path::PathBuf;

use crate::boundary;
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::provenance::{WriteIngress, WriteProvenance, validate_write_provenance};
use crate::schema::{AtmMessageId, InboxMessage, authenticated_source_host, peer_outbound_host};
use crate::send::{SendMessageSource, SendOutcome, SendRequest, WriteOutcome};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, ChatId, CommandAction, HostName, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Parameters for acknowledging one pending-ack mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_chat_id: Option<ChatId>,
    pub caller_team: TeamName,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

impl AckRequest {
    pub fn into_write_request(self) -> SendRequest {
        SendRequest {
            home_dir: self.home_dir,
            current_dir: self.current_dir,
            caller_identity: self.caller_identity,
            caller_chat_id: self.caller_chat_id,
            caller_team: self.caller_team,
            authenticated_source_host: None,
            origin_message_id: None,
            origin_timestamp: None,
            to: None,
            message_source: SendMessageSource::Inline(self.reply_body),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: Some(self.message_id),
            dry_run: false,
        }
    }

    pub fn from_unresolved_write(request: SendRequest) -> Result<Self, AtmError> {
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let SendMessageSource::Inline(reply_body) = request.message_source else {
            return Err(AtmError::validation(
                "acknowledgement reply body must be inline",
            ));
        };
        Ok(Self {
            home_dir: request.home_dir,
            current_dir: request.current_dir,
            caller_identity: request.caller_identity,
            caller_chat_id: request.caller_chat_id,
            caller_team: request.caller_team,
            message_id,
            reply_body,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReplyDisposition {
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
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
    pub reply_disposition: AckReplyDisposition,
    pub reply_text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::send::WarningEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplyTarget {
    agent: AgentName,
    team: TeamName,
    host: Option<HostName>,
}

impl ReplyTarget {
    fn new(agent: AgentName, team: TeamName, host: Option<HostName>) -> Self {
        Self { agent, team, host }
    }
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.agent, self.team)?;
        if let Some(host) = &self.host {
            write!(f, ".{host}")?;
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
        let address: crate::address::AgentAddress =
            value.parse().map_err(serde::de::Error::custom)?;
        let team = address
            .team()
            .cloned()
            .ok_or_else(|| serde::de::Error::custom("expected <agent>@<team> reply target"))?;
        Ok(Self::new(
            address.agent().clone(),
            team,
            address.host().cloned(),
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
    match crate::send::write_mail_with_runtime(
        request.into_write_request(),
        observability,
        runtime,
    )? {
        WriteOutcome::Acknowledged(outcome) => Ok(outcome),
        WriteOutcome::Sent(_) => Err(AtmError::validation(
            "acknowledgement command produced a non-acknowledgement write outcome",
        )),
    }
}

/// Ack-specific source lookup is normalization only. The returned request is
/// then executed by the one canonical write pipeline shared with `atm send`.
pub(crate) struct ResolvedAcknowledgement {
    canonical_request: SendRequest,
    source_record: boundary::Message,
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
}

pub(crate) fn resolve_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    runtime: &R,
) -> Result<ResolvedAcknowledgement, AtmError> {
    validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let request = AckRequest::from_unresolved_write(request)?;
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
    ensure_ack_is_pending(request.message_id, &source_record.envelope)?;
    let reply_target = validate_reply_target(runtime, &source_record, &team)?;
    let canonical_request =
        canonical_ack_write_request(&request, &actor, &team, &reply_target, &source_record)?;
    Ok(ResolvedAcknowledgement {
        canonical_request,
        source_record,
        actor,
        team,
        reply_target,
        reply_text: request.reply_body,
        acknowledged_message_id: request.message_id,
    })
}

/// Resolve an acknowledgement that already arrived through authenticated peer
/// ingress. Its destination is canonical wire data, not a client-supplied
/// local-CLI destination, so this reloads only the local source state that
/// must transition after the received immutable reply persists.
pub(crate) fn resolve_received_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    runtime: &R,
) -> Result<ResolvedAcknowledgement, AtmError> {
    validate_write_provenance(
        WriteIngress::Peer,
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let message_id = request.acknowledges_message_id.ok_or_else(|| {
        AtmError::validation("acknowledgement write is missing acknowledges_message_id")
    })?;
    let target = request.to.clone().ok_or_else(|| {
        AtmError::validation("received peer acknowledgement is missing a destination")
    })?;
    let actor = target.agent().clone();
    let team = target
        .team()
        .cloned()
        .unwrap_or_else(|| request.caller_team.clone());
    let source = load_ack_source(runtime, &request.home_dir, &team, &actor, message_id)?;
    let source_record =
        load_ack_source_record(runtime, &request.home_dir, &team, &actor, &source.row)?;
    ensure_ack_is_pending(message_id, &source_record.envelope)?;
    let reply_target = ReplyTarget::new(actor.clone(), team.clone(), target.host().cloned());
    Ok(ResolvedAcknowledgement {
        canonical_request: request,
        source_record,
        actor,
        team,
        reply_target,
        reply_text: String::new(),
        acknowledged_message_id: message_id,
    })
}

impl ResolvedAcknowledgement {
    pub(crate) fn request(&self) -> SendRequest {
        self.canonical_request.clone()
    }

    /// Builds the acknowledged source replacement that is committed in the
    /// same SQLite writer transaction as the immutable acknowledgement reply.
    pub(crate) fn source_update(&self) -> boundary::Message {
        let timestamp = IsoTimestamp::now();
        let mut source = self.source_record.clone();
        source.envelope.read = true;
        source.envelope.pending_ack_at = None;
        source.envelope.acknowledged_at = Some(timestamp);
        source
    }

    /// Receiver-side acknowledgement mutation occurs only after the canonical
    /// write succeeded. A failed write leaves the source pending.
    pub(crate) fn finish<R: RetainedMailboxRuntime>(
        self,
        _runtime: &R,
        observability: &dyn ObservabilityPort,
        send_outcome: SendOutcome,
    ) -> Result<AckOutcome, AtmError> {
        let outcome = AckOutcome {
            action: CommandAction::Ack,
            team: self.team.clone(),
            agent: self.actor.clone(),
            message_id: self.acknowledged_message_id,
            task_id: self.source_record.envelope.task_id.clone(),
            reply_disposition: AckReplyDisposition::Sent {
                reply_message_id: send_outcome.message_id,
                reply_target: self.reply_target,
            },
            reply_text: self.reply_text,
            warnings: send_outcome.warnings,
        };
        record_ack_telemetry(
            observability,
            &self.actor,
            self.team,
            outcome.message_id,
            outcome.task_id.clone(),
        );
        Ok(outcome)
    }
}

fn canonical_ack_write_request(
    request: &AckRequest,
    actor: &AgentName,
    team: &TeamName,
    target: &ReplyTarget,
    source: &boundary::Message,
) -> Result<SendRequest, AtmError> {
    Ok(SendRequest {
        home_dir: request.home_dir.clone(),
        current_dir: request.current_dir.clone(),
        caller_identity: actor.clone(),
        caller_chat_id: request.caller_chat_id.clone(),
        caller_team: team.clone(),
        authenticated_source_host: None,
        origin_message_id: None,
        origin_timestamp: None,
        to: Some(crate::address::AgentAddress::new(
            target.agent.clone(),
            source.envelope.source_chat_id.clone(),
            Some(target.team.clone()),
            target.host.clone(),
        )?),
        message_source: SendMessageSource::Inline(request.reply_body.clone()),
        summary_override: None,
        requires_ack: false,
        task_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        acknowledges_message_id: Some(request.message_id),
        dry_run: false,
    })
}

struct LoadedAckSource {
    row: boundary::MailStoreMailboxMetadataRow,
}

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    _recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::agent_not_found(agent, team));
    }
    Ok(())
}

fn load_ack_source<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    message_id: AtmMessageId,
) -> Result<LoadedAckSource, AtmError> {
    let rows = runtime.query_mailbox_metadata_rows(home_dir, team, actor, None)?;
    let row = rows
        .iter()
        .find(|row| row.message_id == Some(message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {message_id} was not found in {actor}@{team}"
            ))
        })?;
    if rows
        .iter()
        .any(|candidate| candidate.parent_message_id == Some(message_id))
    {
        return Err(AtmError::validation(format!(
            "message {message_id} has been updated; acknowledge the current terminal message instead"
        )));
    }
    Ok(LoadedAckSource { row: row.clone() })
}

fn load_ack_source_record<R: RetainedMailboxRuntime>(
    runtime: &R,
    home_dir: &std::path::Path,
    team: &TeamName,
    actor: &AgentName,
    row: &boundary::MailStoreMailboxMetadataRow,
) -> Result<boundary::Message, AtmError> {
    runtime
        .load_message_record(home_dir, team, actor, &row.message_key)?
        .ok_or_else(|| {
            AtmError::validation("acknowledgement source metadata could not be reloaded")
        })
}

fn ensure_ack_is_pending(message_id: AtmMessageId, source: &InboxMessage) -> Result<(), AtmError> {
    if source.pending_ack_at.is_some() {
        return Ok(());
    }
    let state = if source.acknowledged_at.is_some() {
        "already acknowledged"
    } else {
        "not pending acknowledgement"
    };
    Err(AtmError::validation(format!(
        "message {message_id} is {state}"
    )))
}

fn validate_reply_target<R: RetainedServiceRuntime>(
    runtime: &R,
    source: &boundary::Message,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let team = source
        .envelope
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let agent = crate::threading::canonical_sender_identity(&source.envelope);
    // A normal peer receipt carries authenticated provenance. A same-store
    // peer receipt deliberately retains the origin row instead of writing a
    // duplicate, so its reply target comes from that row's retained outbound
    // host. Neither case recreates a hostless reply address.
    let host = reply_target_host(&source.envelope)?;
    if host.is_none() {
        ensure_roster_member_exists(
            runtime,
            &team,
            &agent,
            "reload the roster before retrying the acknowledgement",
        )?;
    }
    Ok(ReplyTarget::new(agent, team, host))
}

fn reply_target_host(source: &InboxMessage) -> Result<Option<crate::types::HostName>, AtmError> {
    let authenticated = authenticated_source_host(source)?;
    let outbound = peer_outbound_host(source)?;
    let validated = validate_write_provenance(
        WriteIngress::Canonical,
        WriteProvenance {
            target_host: outbound.as_ref(),
            authenticated_source_host: authenticated.as_ref(),
            origin_message_id: authenticated.is_some(),
            origin_timestamp: authenticated.is_some(),
        },
    )?;
    Ok(validated
        .is_authenticated_peer()
        .then_some(authenticated)
        .flatten()
        .or(outbound))
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
        tracing::warn!(%error, command = "ack", "failed to emit acknowledgement telemetry");
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::Map;

    use super::{AckRequest, ReplyTarget, canonical_ack_write_request, reply_target_host};
    use crate::boundary::{Message, MessageKey};
    use crate::schema::{
        AckIntentFields, AtmMessageId, InboxMessage, authenticated_source_host,
        set_authenticated_source_host, set_peer_outbound_write,
    };
    use crate::types::{AgentName, ChatId, HostName, IsoTimestamp, TeamName};

    #[test]
    fn remote_ack_is_the_canonical_host_qualified_write() {
        let message_id = AtmMessageId::new();
        let mut envelope = InboxMessage {
            from: "remote-agent".parse().expect("agent"),
            source_chat_id: None,
            text: "request acknowledgement".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("remote-team".parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(message_id),
            requires_ack: AckIntentFields::required_pending(IsoTimestamp::now()).requires_ack,
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };
        let host: HostName = "peer.example.test".parse().expect("host");
        set_authenticated_source_host(&mut envelope, Some(host.clone()));
        assert_eq!(
            authenticated_source_host(&envelope).expect("stored authenticated host"),
            Some(host.clone())
        );
        let source = Message {
            team: "local-team".parse().expect("team"),
            agent: "local-agent".parse().expect("agent"),
            message_key: MessageKey::new("ack-source").expect("key"),
            envelope,
        };
        let request = AckRequest {
            home_dir: PathBuf::from("/tmp/atm-test"),
            current_dir: PathBuf::from("/tmp/atm-test"),
            caller_identity: "local-agent".parse().expect("agent"),
            caller_chat_id: Some("chat-42".parse::<ChatId>().expect("chat id")),
            caller_team: "local-team".parse().expect("team"),
            message_id,
            reply_body: "acknowledged".to_string(),
        };
        let target = ReplyTarget::new(
            "remote-agent".parse::<AgentName>().expect("agent"),
            "remote-team".parse::<TeamName>().expect("team"),
            Some(host.clone()),
        );

        let write = canonical_ack_write_request(
            &request,
            &request.caller_identity,
            &request.caller_team,
            &target,
            &source,
        )
        .expect("canonical ack write");
        assert_eq!(write.to.expect("destination").host(), Some(&host));
        assert_eq!(write.acknowledges_message_id, Some(message_id));
        assert_eq!(
            write.caller_chat_id.as_ref().map(ChatId::as_str),
            Some("chat-42")
        );
        assert_eq!(
            target.to_string(),
            "remote-agent@remote-team.peer.example.test"
        );
    }

    #[test]
    fn same_store_receipt_uses_retained_origin_host_for_ack_reply() {
        let host: HostName = "192.168.128.82".parse().expect("host");
        let mut envelope = InboxMessage {
            from: "remote-agent".parse().expect("agent"),
            source_chat_id: None,
            text: "request acknowledgement".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some("remote-team".parse().expect("team")),
            destination_chat_id: None,
            summary: None,
            message_id: Some(AtmMessageId::new()),
            requires_ack: true,
            pending_ack_at: Some(IsoTimestamp::now()),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        };
        set_peer_outbound_write(&mut envelope, &host, "{}".to_string());

        assert_eq!(
            reply_target_host(&envelope).expect("reply host"),
            Some(host)
        );
    }
}
