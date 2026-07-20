use std::path::PathBuf;

use crate::boundary;
use crate::error::AtmError;
use crate::read::state;
use crate::schema::{AtmMessageId, remote_host as message_remote_host};
use crate::send::{RemoteTargetHost, SendMessageSource, SendRequest, input, summary};
use crate::service_runtime::LocalServiceRuntime;
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Parameters supplied by the `atm ack` convenience command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub message_id: AtmMessageId,
    pub reply_body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AckReplyDisposition {
    Sent {
        reply_message_id: AtmMessageId,
        reply_target: ReplyTarget,
    },
}

/// CLI/graft acknowledgement presentation derived from the one send result.
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
    remote_host: Option<String>,
}

impl std::fmt::Display for ReplyTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}@{}", self.agent, self.team)?;
        if let Some(host) = &self.remote_host {
            write!(formatter, ".{host}")?;
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
        let (team, remote_host) = team_and_host
            .split_once('.')
            .map_or((team_and_host, None), |(team, host)| {
                (team, Some(host.to_string()))
            });
        Ok(Self {
            agent: agent.parse().map_err(serde::de::Error::custom)?,
            team: team.parse().map_err(serde::de::Error::custom)?,
            remote_host,
        })
    }
}

impl AckOutcome {
    pub fn from_send_outcome(outcome: crate::send::SendOutcome, request: &AckRequest) -> Self {
        Self {
            action: CommandAction::Ack,
            team: request.caller_team.clone(),
            agent: request.caller_identity.clone(),
            message_id: request.message_id,
            task_id: outcome.task_id,
            reply_disposition: AckReplyDisposition::Sent {
                reply_message_id: outcome.message_id,
                reply_target: ReplyTarget {
                    agent: outcome.agent,
                    team: outcome.team,
                    remote_host: None,
                },
            },
            reply_text: outcome
                .message
                .unwrap_or_else(|| request.reply_body.clone()),
            warnings: outcome.warnings,
        }
    }
}

/// Converts the convenience command input into the sole outbound request shape.
pub fn prepare_ack_send_request(request: AckRequest) -> Result<SendRequest, AtmError> {
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let mut send = SendRequest::new(
        request.home_dir,
        request.current_dir,
        actor.clone(),
        &format!("{actor}@{team}"),
        team,
        SendMessageSource::Inline(reply_text.clone()),
        Some(summary::build_summary(&reply_text, None)),
        false,
        None,
        false,
    )?;
    send.acknowledges_message_id = Some(request.message_id);
    Ok(send)
}

/// Convenience API only: it delegates to the canonical send pipeline and has
/// no acknowledgement-specific persistence or delivery implementation.
pub fn ack_mail(
    request: AckRequest,
    observability: &dyn crate::observability::ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let runtime = default_runtime()?;
    let original = request.clone();
    let (send, mutation) = resolve_ack_send_request(&runtime, prepare_ack_send_request(request)?)?;
    let outcome = crate::send::send_mail_with_runtime(send, observability, &runtime)?;
    if matches!(outcome.outcome, crate::send::SendCommandOutcome::Sent) {
        commit_ack_mutation(&runtime, mutation)?;
    }
    Ok(AckOutcome::from_send_outcome(outcome, &original))
}

pub struct AckMutation {
    actor: AgentName,
    team: TeamName,
    message_key: boundary::MessageKey,
    expires_at: Option<IsoTimestamp>,
}

/// Resolve an ack convenience request into the same `SendRequest` used by every
/// other outbound message. No delivery or nudge occurs here.
pub fn resolve_ack_send_request(
    runtime: &LocalServiceRuntime,
    request: SendRequest,
) -> Result<(SendRequest, AckMutation), AtmError> {
    let message_id = request.acknowledges_message_id.ok_or_else(|| {
        AtmError::validation("ack send is missing acknowledges_message_id".to_string())
    })?;
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    let (row, source) = load_pending_ack_source(runtime, &request, &team, &actor, message_id)?;
    let (recipient, recipient_team, remote_host) =
        resolve_ack_recipient(runtime, &source.envelope, &actor, &team)?;
    let body = ack_reply_body(request.message_source.clone())?;
    let mut resolved = build_ack_send_request(
        request,
        &actor,
        &team,
        recipient,
        recipient_team,
        &source.envelope,
        body,
    )?;
    resolved.acknowledges_message_id = Some(message_id);
    resolved.remote_host = remote_host
        .map(|host| RemoteTargetHost::parse(&host))
        .transpose()?;
    Ok((
        resolved,
        AckMutation {
            actor,
            team,
            message_key: row.message_key.clone(),
            expires_at: source.envelope.expires_at,
        },
    ))
}

fn load_pending_ack_source(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    team: &TeamName,
    actor: &AgentName,
    message_id: AtmMessageId,
) -> Result<(boundary::MailStoreMailboxMetadataRow, boundary::Message), AtmError> {
    let rows = runtime.query_mailbox_metadata_rows(&request.home_dir, team, actor, None)?;
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
        .any(|row| row.parent_message_id == Some(message_id))
    {
        return Err(AtmError::validation(format!(
            "message {message_id} has been updated; acknowledge the current terminal message instead"
        )));
    }
    let source = runtime
        .load_message_record(&request.home_dir, team, actor, &row.message_key)?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {message_id} metadata could not be reloaded from sqlite"
            ))
        })?;
    if !matches!(
        state::derive_ack_state(&source.envelope),
        crate::types::AckState::PendingAck
    ) {
        return Err(AtmError::validation(format!(
            "message {message_id} is not pending acknowledgement"
        )));
    }
    Ok((row.clone(), source))
}

fn resolve_ack_recipient(
    runtime: &LocalServiceRuntime,
    source: &crate::schema::InboxMessage,
    actor: &AgentName,
    team: &TeamName,
) -> Result<(AgentName, TeamName, Option<String>), AtmError> {
    let recipient = crate::threading::canonical_sender_identity(source);
    let recipient_team = source.source_team.clone().unwrap_or_else(|| team.clone());
    let remote_host = message_remote_host(source).map(str::to_owned);
    if remote_host.is_none() && recipient == *actor && recipient_team == *team {
        return Err(AtmError::validation(
            "local self-ack is not allowed without an explicit host target".to_string(),
        ));
    }
    if remote_host.is_none()
        && runtime
            .load_roster_member(&recipient_team, &recipient)?
            .is_none()
    {
        return Err(AtmError::agent_not_found(&recipient, &recipient_team));
    }
    Ok((recipient, recipient_team, remote_host))
}

fn ack_reply_body(source: SendMessageSource) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(body)
        | SendMessageSource::File {
            message: Some(body),
            ..
        } => Ok(body),
        SendMessageSource::File { message: None, .. } => Err(AtmError::validation(
            "ack reply body was not materialized".to_string(),
        )),
    }
}

fn build_ack_send_request(
    request: SendRequest,
    actor: &AgentName,
    team: &TeamName,
    recipient: AgentName,
    recipient_team: TeamName,
    source: &crate::schema::InboxMessage,
    body: String,
) -> Result<SendRequest, AtmError> {
    SendRequest::new(
        request.home_dir,
        request.current_dir,
        actor.clone(),
        &format!("{recipient}@{recipient_team}"),
        team.clone(),
        SendMessageSource::Inline(body.clone()),
        Some(summary::build_summary(&body, None)),
        false,
        source.task_id.clone(),
        false,
    )
}

/// Commit the local pending-ack mutation only after the canonical send reports
/// confirmed delivery.
pub fn commit_ack_mutation(
    runtime: &LocalServiceRuntime,
    mutation: AckMutation,
) -> Result<(), AtmError> {
    let timestamp = IsoTimestamp::now();
    runtime.persist_message_state(boundary::MailMessageState {
        team: mutation.team,
        agent: mutation.actor.clone(),
        actor: mutation.actor,
        message_key: mutation.message_key,
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(timestamp),
        expires_at: mutation.expires_at,
        deleted_at: None,
        updated_at: Some(timestamp),
    })
}
