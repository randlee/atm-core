use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::boundary;
use crate::caller_context::ActivityObservation;
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::provenance::{WriteIngress, WriteProvenance, validate_write_provenance};
use crate::schema::{AtmMessageId, InboxMessage, authenticated_source_host, peer_delivery_target};
use crate::send::{SendMessageSource, SendOutcome, SendRequest, WriteOutcome};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, ChatId, CommandAction, HostName, IsoTimestamp, TaskId, TeamName};
use atm_storage::contract::{
    AcknowledgementReplyBuilder, AcknowledgementSource, Message as StoredMessage, MessageKey,
};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_observation: Option<ActivityObservation>,
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
            activity_observation: self.activity_observation,
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
            activity_observation: request.activity_observation,
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
    /// Receipt metadata carried only on the local daemon protocol response.
    /// The CLI/graft caller combines it with its original `AckRequest` to
    /// deliver the exact host-qualified receipt after local persistence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    peer_receipt: Option<PeerAckReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::send::WarningEntry>,
}

impl AckOutcome {
    /// Reconstruct the canonical direct-peer receipt from the original local
    /// acknowledgement request. The persisted identifier and timestamp always
    /// come from the daemon response, never from the caller.
    pub fn peer_receipt_request(
        &self,
        acknowledgement: &AckRequest,
    ) -> Result<Option<SendRequest>, AtmError> {
        let Some(receipt) = &self.peer_receipt else {
            return Ok(None);
        };
        let AckReplyDisposition::Sent { reply_target, .. } = &self.reply_disposition;
        let destination = crate::address::AgentAddress::new(
            reply_target.agent.clone(),
            receipt.target_chat_id.clone(),
            Some(reply_target.team.clone()),
            reply_target.host.clone(),
        )?;
        Ok(Some(SendRequest {
            home_dir: acknowledgement.home_dir.clone(),
            current_dir: acknowledgement.current_dir.clone(),
            caller_identity: acknowledgement.caller_identity.clone(),
            caller_chat_id: acknowledgement.caller_chat_id.clone(),
            caller_team: acknowledgement.caller_team.clone(),
            activity_observation: acknowledgement.activity_observation.clone(),
            authenticated_source_host: None,
            origin_message_id: Some(receipt.reply_message_id),
            origin_timestamp: Some(receipt.reply_timestamp),
            to: Some(destination),
            message_source: SendMessageSource::Inline(self.reply_text.clone()),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            acknowledges_message_id: Some(self.message_id),
            dry_run: false,
        }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PeerAckReceipt {
    reply_message_id: AtmMessageId,
    reply_timestamp: IsoTimestamp,
    target_chat_id: Option<ChatId>,
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

/// Immutable acknowledgement outcome assembled by the sealed storage
/// transaction, then completed through the canonical post-write pipeline.
#[derive(Clone)]
pub(crate) struct ResolvedAcknowledgement {
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
    peer_receipt: Option<PeerAckReceipt>,
}

/// Reply data retained only long enough to form the post-commit route and
/// acknowledgement response.  The source itself is loaded by the storage
/// writer, never by the application-layer admission path.
#[derive(Clone)]
pub(crate) struct AtomicAcknowledgementWrite {
    pub(crate) reply: StoredMessage,
    pub(crate) canonical_request: SendRequest,
    pub(crate) acknowledgement: ResolvedAcknowledgement,
}

#[derive(Clone)]
enum AtomicAcknowledgementKind {
    Local(Box<AckRequest>),
    Received(Box<SendRequest>),
}

struct AtomicAcknowledgementBuilder {
    kind: AtomicAcknowledgementKind,
    // The storage callback requires `&self`; this narrow mutex publishes one
    // immutable reply assembled exactly once across that callback boundary.
    built: Mutex<Option<AtomicAcknowledgementWrite>>,
}

impl AtomicAcknowledgementBuilder {
    fn new(kind: AtomicAcknowledgementKind) -> Self {
        Self {
            kind,
            built: Mutex::new(None),
        }
    }

    fn take(&self) -> Result<AtomicAcknowledgementWrite, AtmError> {
        self.built
            .lock()
            .map_err(|_| {
                AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
            })?
            .take()
            .ok_or_else(|| {
                AtmError::daemon_unavailable(
                    "acknowledgement transaction completed without a reply",
                )
            })
    }
}

impl AcknowledgementReplyBuilder for AtomicAcknowledgementBuilder {
    fn build_reply(&self, source: &StoredMessage) -> Result<StoredMessage, AtmError> {
        let built = match &self.kind {
            AtomicAcknowledgementKind::Local(request) => {
                let actor = request.caller_identity.clone();
                let team = request.caller_team.clone();
                let reply_target = reply_target_from_source(&source.envelope, &team)?;
                let canonical_request = canonical_ack_write_request(
                    request,
                    &actor,
                    &team,
                    &reply_target,
                    &boundary::Message {
                        team: source.team.clone(),
                        agent: source.agent.clone(),
                        message_key: source.message_key.clone(),
                        envelope: source.envelope.clone(),
                    },
                )?;
                build_atomic_acknowledgement(
                    canonical_request,
                    actor,
                    team,
                    reply_target,
                    request.reply_body.clone(),
                    request.message_id,
                    source.envelope.task_id.clone(),
                )?
            }
            AtomicAcknowledgementKind::Received(request) => {
                let target = request.to.clone().ok_or_else(|| {
                    AtmError::validation("received peer acknowledgement is missing a destination")
                })?;
                let actor = target.agent().clone();
                let team = target
                    .team()
                    .cloned()
                    .unwrap_or_else(|| request.caller_team.clone());
                let reply_target =
                    ReplyTarget::new(actor.clone(), team.clone(), target.host().cloned());
                let message_id = request.acknowledges_message_id.ok_or_else(|| {
                    AtmError::validation("acknowledgement write is missing acknowledges_message_id")
                })?;
                let reply_text = match &request.message_source {
                    SendMessageSource::Inline(value) => value.clone(),
                    SendMessageSource::File { .. } => {
                        return Err(AtmError::validation(
                            "acknowledgement reply body must be inline",
                        ));
                    }
                };
                build_atomic_acknowledgement(
                    *request.clone(),
                    actor,
                    team,
                    reply_target,
                    reply_text,
                    message_id,
                    source.envelope.task_id.clone(),
                )?
            }
        };
        let mut slot = self.built.lock().map_err(|_| {
            AtmError::daemon_unavailable("acknowledgement builder state lock poisoned")
        })?;
        *slot = Some(built.clone());
        Ok(built.reply)
    }
}

pub(crate) fn admit_acknowledgement_write<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    runtime: &R,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime.acknowledge_message_atomically(&source, builder.clone())?;
    builder.take()
}

/// Async counterpart of [`admit_acknowledgement_write`] for the replacement
/// Tokio daemon. The roster check remains synchronous core validation; the
/// source lookup, reply creation, and atomic source transition are one await
/// on the storage-owned durable-admission lane.
pub(crate) async fn admit_acknowledgement_write_async(
    request: SendRequest,
    runtime: &LocalServiceRuntime,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    let provenance = validate_write_provenance(
        if request.to.is_some() {
            WriteIngress::Peer
        } else {
            WriteIngress::Canonical
        },
        WriteProvenance {
            target_host: request.to.as_ref().and_then(|address| address.host()),
            authenticated_source_host: request.authenticated_source_host.as_ref(),
            origin_message_id: request.origin_message_id.is_some(),
            origin_timestamp: request.origin_timestamp.is_some(),
        },
    )?;
    let (source, builder) = if let Some(target) = request.to.as_ref() {
        if !provenance.is_authenticated_peer() {
            return Err(AtmError::validation(
                "acknowledgement write must not include a client-supplied destination",
            ));
        }
        let message_id = request.acknowledges_message_id.ok_or_else(|| {
            AtmError::validation("acknowledgement write is missing acknowledges_message_id")
        })?;
        let team = target
            .team()
            .cloned()
            .unwrap_or_else(|| request.caller_team.clone());
        (
            AcknowledgementSource {
                team,
                agent: target.agent().clone(),
                message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Received(Box::new(request)),
            )),
        )
    } else {
        let request = AckRequest::from_unresolved_write(request)?;
        ensure_roster_member_exists(
            runtime,
            &request.caller_team,
            &request.caller_identity,
            "Repair or reload the ATM roster before retrying `atm ack`.",
        )?;
        (
            AcknowledgementSource {
                team: request.caller_team.clone(),
                agent: request.caller_identity.clone(),
                message_id: request.message_id,
            },
            Arc::new(AtomicAcknowledgementBuilder::new(
                AtomicAcknowledgementKind::Local(Box::new(request)),
            )),
        )
    };
    let _commit = runtime
        .acknowledge_message_atomically_async(source, builder.clone())
        .await?;
    builder.take()
}

fn build_atomic_acknowledgement(
    canonical_request: SendRequest,
    actor: AgentName,
    team: TeamName,
    reply_target: ReplyTarget,
    reply_text: String,
    acknowledged_message_id: AtmMessageId,
    source_task_id: Option<TaskId>,
) -> Result<AtomicAcknowledgementWrite, AtmError> {
    if canonical_request.authenticated_source_host.is_none()
        && reply_target.host.is_none()
        && actor == reply_target.agent
        && team == reply_target.team
    {
        return Err(AtmError::self_addressed_send_invalid(format!(
            "self-addressed messages are invalid ATM input: '{actor}@{team}' may not send to itself"
        )));
    }
    let destination = canonical_request.to.as_ref().ok_or_else(|| {
        AtmError::validation("acknowledgement reply is missing a canonical destination")
    })?;
    let message_id = canonical_request.origin_message_id.unwrap_or_default();
    let timestamp = canonical_request
        .origin_timestamp
        .unwrap_or_else(IsoTimestamp::now);
    let summary = crate::send::summary::build_summary(&reply_text, None);
    let mut envelope = InboxMessage {
        from: actor.clone(),
        source_chat_id: canonical_request.caller_chat_id.clone(),
        text: reply_text.clone(),
        timestamp,
        read: false,
        source_team: Some(team.clone()),
        destination_chat_id: destination.chat_id().cloned(),
        summary: Some(summary.clone()),
        message_id: Some(message_id),
        requires_ack: false,
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(acknowledged_message_id),
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: serde_json::Map::new(),
    };
    persist_direct_peer_target(&canonical_request, destination, &mut envelope);
    let reply = StoredMessage {
        team: destination.team().cloned().ok_or_else(|| {
            AtmError::validation("acknowledgement reply destination is missing a team")
        })?,
        agent: destination.agent().clone(),
        message_key: MessageKey::from(message_id),
        envelope,
    };
    let peer_receipt = reply_target.host.as_ref().map(|_| PeerAckReceipt {
        reply_message_id: message_id,
        reply_timestamp: timestamp,
        target_chat_id: destination.chat_id().cloned(),
    });
    let acknowledgement = ResolvedAcknowledgement {
        actor,
        team,
        reply_target,
        reply_text,
        acknowledged_message_id,
        source_task_id,
        peer_receipt,
    };
    Ok(AtomicAcknowledgementWrite {
        reply,
        canonical_request,
        acknowledgement,
    })
}

fn persist_direct_peer_target(
    canonical_request: &SendRequest,
    destination: &crate::address::AgentAddress,
    envelope: &mut InboxMessage,
) {
    if let Some(host) = crate::send::direct_peer_destination(canonical_request, destination) {
        crate::schema::set_peer_delivery_target(envelope, &host);
    }
}

fn reply_target_from_source(
    source: &InboxMessage,
    current_team: &TeamName,
) -> Result<ReplyTarget, AtmError> {
    let team = source
        .source_team
        .clone()
        .unwrap_or_else(|| current_team.clone());
    let agent = crate::threading::canonical_sender_identity(source);
    Ok(ReplyTarget::new(agent, team, reply_target_host(source)?))
}

impl ResolvedAcknowledgement {
    pub(crate) fn source_task_id(&self) -> Option<TaskId> {
        self.source_task_id.clone()
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
            task_id: self.source_task_id.clone(),
            reply_disposition: AckReplyDisposition::Sent {
                reply_message_id: send_outcome.message_id,
                reply_target: self.reply_target,
            },
            reply_text: self.reply_text,
            peer_receipt: self.peer_receipt,
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
        activity_observation: request.activity_observation.clone(),
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

fn ensure_roster_member_exists<R: RetainedServiceRuntime>(
    runtime: &R,
    team: &TeamName,
    agent: &AgentName,
    recovery: &str,
) -> Result<(), AtmError> {
    if runtime.load_roster_member(team, agent)?.is_none() {
        return Err(AtmError::new(
            crate::error_codes::AtmErrorCode::AgentNotFound,
            format!("agent '{agent}' was not found in team '{team}'\n  Recovery: {recovery}"),
        ));
    }
    Ok(())
}

fn reply_target_host(source: &InboxMessage) -> Result<Option<crate::types::HostName>, AtmError> {
    let authenticated = authenticated_source_host(source)?;
    let outbound = peer_delivery_target(source)?;
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

    use super::{
        AckRequest, ReplyTarget, build_atomic_acknowledgement, canonical_ack_write_request,
        reply_target_host,
    };
    use crate::boundary::{Message, MessageKey};
    use crate::caller_context::ActivityObservation;
    use crate::read::{MailboxQueryFilters, ReadQuery};
    use crate::schema::{
        AckIntentFields, AtmMessageId, InboxMessage, authenticated_source_host,
        set_authenticated_source_host, set_peer_delivery_target,
    };
    use crate::send::{SendMessageSource, WriteRequest};
    use crate::types::{
        AgentName, ChatId, HostName, IsoTimestamp, ReadSelection, SessionId, TeamName,
    };

    #[test]
    fn request_json_omits_or_includes_activity_observation() {
        let observation = ActivityObservation {
            team: "local-team".parse().expect("team"),
            member: "local-agent".parse().expect("agent"),
            session_id: Some(SessionId::new("session-17").expect("session")),
            pid: Some(17),
        };
        let write = WriteRequest::new(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            "local-agent".parse().expect("agent"),
            "remote@remote-team",
            "local-team".parse().expect("team"),
            SendMessageSource::Inline("body".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect("write");
        assert!(
            serde_json::to_value(&write)
                .expect("json")
                .get("activity_observation")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(write.with_activity_observation(Some(observation.clone())))
                .expect("json")["activity_observation"]["pid"],
            17
        );
        let query = ReadQuery::from_filters(
            PathBuf::from("/tmp"),
            PathBuf::from("/tmp"),
            "local-agent".parse().expect("agent"),
            "local-team".parse().expect("team"),
            ReadSelection::All,
            false,
            false,
            MailboxQueryFilters::default(),
        )
        .expect("query");
        assert!(
            serde_json::to_value(&query)
                .expect("json")
                .get("activity_observation")
                .is_none()
        );
        assert_eq!(
            serde_json::to_value(query.with_activity_observation(Some(observation))).expect("json")
                ["activity_observation"]["session_id"],
            "session-17"
        );
    }

    #[test]
    fn acknowledgement_round_trip_preserves_activity_observation() {
        let observation = ActivityObservation {
            team: "local-team".parse().expect("team"),
            member: "local-agent".parse().expect("agent"),
            session_id: Some(SessionId::new("session-17").expect("session")),
            pid: Some(17),
        };
        let temp_dir = std::env::temp_dir();
        let request = AckRequest {
            home_dir: temp_dir.clone(),
            current_dir: temp_dir,
            caller_identity: "local-agent".parse().expect("agent"),
            caller_chat_id: None,
            caller_team: "local-team".parse().expect("team"),
            activity_observation: Some(observation.clone()),
            message_id: AtmMessageId::new(),
            reply_body: "ack".to_string(),
        };
        let write = request.clone().into_write_request();
        assert_eq!(write.activity_observation, Some(observation));
        assert_eq!(
            AckRequest::from_unresolved_write(write)
                .expect("round trip")
                .activity_observation,
            request.activity_observation
        );
    }

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
        let temp_dir = std::env::temp_dir();
        let request = AckRequest {
            home_dir: temp_dir.clone(),
            current_dir: temp_dir,
            caller_identity: "local-agent".parse().expect("agent"),
            caller_chat_id: Some("chat-42".parse::<ChatId>().expect("chat id")),
            caller_team: "local-team".parse().expect("team"),
            activity_observation: None,
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
        assert_eq!(write.to.as_ref().expect("destination").host(), Some(&host));
        assert_eq!(write.acknowledges_message_id, Some(message_id));
        assert_eq!(
            write.caller_chat_id.as_ref().map(ChatId::as_str),
            Some("chat-42")
        );
        assert_eq!(
            target.to_string(),
            "remote-agent@remote-team.peer.example.test"
        );

        let acknowledged = build_atomic_acknowledgement(
            write,
            request.caller_identity.clone(),
            request.caller_team.clone(),
            target,
            request.reply_body.clone(),
            message_id,
            None,
        )
        .expect("acknowledgement write");
        let acknowledgement_id = acknowledged
            .reply
            .envelope
            .message_id
            .expect("acknowledgement ULID");
        assert_ne!(
            acknowledgement_id, message_id,
            "the acknowledgement is a new immutable write, not a replay of its send"
        );
        assert_eq!(
            acknowledged.reply.envelope.acknowledges_message_id,
            Some(message_id),
            "the acknowledgement response keeps the exact send ULID it causally acknowledges"
        );
        let receipt = acknowledged
            .acknowledgement
            .peer_receipt
            .as_ref()
            .expect("cross-host acknowledgement exposes one caller-owned receipt");
        assert_eq!(receipt.reply_message_id, acknowledgement_id);
        assert_eq!(
            receipt.reply_timestamp, acknowledged.reply.envelope.timestamp,
            "caller receipt uses the exact metadata already persisted locally"
        );
        assert_eq!(receipt.target_chat_id, None);
        assert_eq!(
            acknowledged.reply.envelope.extra["peerOutbound"]["host"],
            host.to_string(),
            "the retained direct target is enough for the synchronous router"
        );
        assert!(
            acknowledged.reply.envelope.extra["peerOutbound"]
                .get("request")
                .is_none(),
            "acknowledgements retain no serialized replay payload"
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
        set_peer_delivery_target(&mut envelope, &host);

        assert_eq!(
            reply_target_host(&envelope).expect("reply host"),
            Some(host)
        );
    }
}
