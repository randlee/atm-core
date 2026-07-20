//! Send command service implementation and post-send hook handling.

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use crate::address::AgentAddress;
use crate::boundary;
use crate::boundary::PostSendHookEmitter;
use crate::config;
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::AtmError;
use crate::observability::ObservabilityPort;
use crate::schema::{AtmMessageId, ThreadMode, remote_host as message_remote_host};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use serde::{Deserialize, Serialize};

mod context;
mod delivery_persistence;
pub(crate) mod file_policy;
pub(crate) mod hook;
pub mod input;
#[doc(hidden)]
pub mod nudge_template;
mod outcome;
mod persistence;
mod remote_receipt;
pub(crate) mod summary;
mod target;
mod threading_helpers;
mod warning;

use context::SendExecutionContext;
pub(crate) use context::{persist_send_message, prepare_send_context};
pub(crate) use delivery_persistence::{DeliveryPersistenceDisposition, DeliveryPersistenceResult};
use outcome::finalize_send_outcome;
pub(crate) use persistence::persist_message;
pub use remote_receipt::RemoteDeliveryReceiptStatus;
#[doc(hidden)]
pub use remote_receipt::{
    finalize_remote_delivery_receipt_with_runtime, persist_remote_delivery_receipt_with_runtime,
};
pub use target::{PeerLoopbackHost, qualified_sender_identity, qualified_sender_origin};
pub(crate) use target::{
    ResolvedRecipient, resolve_message_body, resolve_recipient, validate_non_self_recipient,
};
pub(crate) use threading_helpers::prepare_threaded_message;
pub use warning::WarningEntry;

/// Finalize the source mailbox state for a confirmed acknowledgement send.
///
/// An acknowledgement is a normal [`SendRequest`] whose only additional
/// semantic field is `acknowledges_message_id`. The source daemon owns this
/// state update; an inbound remote request has `source_remote_host` set and
/// must only persist the received message.
pub fn finalize_acknowledgement_after_confirmed_send(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    outcome: &mut SendOutcome,
) {
    let Some(message_id) = request.acknowledges_message_id else {
        return;
    };
    if !matches!(outcome.outcome, SendCommandOutcome::Sent) {
        return;
    }

    if let Err(error) = finalize_acknowledgement_after_confirmed_delivery(runtime, request) {
        outcome.warnings.push(WarningEntry::with_code(
            error.code,
            format!(
                "ATM delivered acknowledgement reply {} but could not update local acknowledgement state for {}: {}.",
                outcome.message_id, message_id, error
            ),
            Some("Retry `atm ack` only after repairing the local mailbox state; the sent reply was already delivered."),
        ));
    }
}

/// Apply the source-side state transition after a transport has confirmed an
/// acknowledgement delivery. Both immediate and replayed sends use this one
/// operation; inbound requests are excluded because they only persist mail.
pub fn finalize_acknowledgement_after_confirmed_delivery(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
) -> Result<(), AtmError> {
    let Some(message_id) = request.acknowledges_message_id else {
        return Ok(());
    };
    if request.source_remote_host.is_some() {
        return Ok(());
    }
    persist_acknowledged_source(runtime, request, message_id)
}

fn persist_acknowledged_source(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    message_id: AtmMessageId,
) -> Result<(), AtmError> {
    let (source, message_key, _) = load_acknowledgement_source(runtime, request, message_id)?;
    if !matches!(
        crate::read::state::derive_ack_state(&source.envelope),
        crate::types::AckState::PendingAck
    ) {
        return Ok(());
    }

    let timestamp = IsoTimestamp::now();
    runtime.persist_message_state(boundary::MailMessageState {
        team: request.caller_team.clone(),
        agent: request.caller_identity.clone(),
        actor: request.caller_identity.clone(),
        message_key,
        read: true,
        pending_ack_at: None,
        acknowledged_at: Some(timestamp),
        expires_at: source.envelope.expires_at,
        deleted_at: None,
        updated_at: Some(timestamp),
    })
}

pub(super) const POST_SEND_HOOK_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendMessageSource {
    Inline(String),
    File {
        path: PathBuf,
        message: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RemoteTargetHost(String);

impl RemoteTargetHost {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("remote host must not be empty").with_recovery(
                "Provide a non-empty `<host>` via `atm send <agent>@<team>.<host> ...` or `atm send <agent>@<team> --host <host> ...` before retrying.",
            ));
        }
        if trimmed.chars().any(|ch| {
            matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | '/' | '\\') || ch.is_whitespace()
        }) {
            return Err(AtmError::address_parse(format!(
                "remote host `{trimmed}` contains unsupported characters"
            ))
            .with_recovery(
                "Use one exact host token composed of hostname labels or a literal IP address before retrying the remote send.",
            ));
        }
        Ok(Self(trimmed.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn literal_ip(&self) -> Option<IpAddr> {
        self.as_str().parse::<IpAddr>().ok()
    }

    pub fn targets_loopback(&self) -> bool {
        self.as_str().eq_ignore_ascii_case("localhost")
            || self.literal_ip().is_some_and(|ip| ip.is_loopback())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSendTarget {
    pub to: AgentAddress,
    pub remote_host: Option<RemoteTargetHost>,
}

pub trait SendTargetParser: crate::boundary::sealed::Sealed {
    fn parse_target(
        &self,
        raw_target: &str,
        explicit_host: Option<&str>,
    ) -> Result<ParsedSendTarget, AtmError>;
}

#[derive(Debug, Default)]
pub struct DefaultSendTargetParser;

impl crate::boundary::sealed::Sealed for DefaultSendTargetParser {}

impl SendTargetParser for DefaultSendTargetParser {
    fn parse_target(
        &self,
        raw_target: &str,
        explicit_host: Option<&str>,
    ) -> Result<ParsedSendTarget, AtmError> {
        parse_send_target_impl(raw_target, explicit_host)
    }
}

pub fn parse_send_target(
    raw_target: &str,
    explicit_host: Option<&str>,
) -> Result<ParsedSendTarget, AtmError> {
    DefaultSendTargetParser.parse_target(raw_target, explicit_host)
}

fn parse_send_target_impl(
    raw_target: &str,
    explicit_host: Option<&str>,
) -> Result<ParsedSendTarget, AtmError> {
    let trimmed = raw_target.trim();
    if trimmed.is_empty() {
        return Err(AtmError::address_parse("agent name must not be empty"));
    }

    let Some((raw_agent, raw_team_or_remote)) = trimmed.split_once('@') else {
        validate_send_target_segment(trimmed, "agent")?;
        if explicit_host.is_some() {
            return Err(AtmError::address_parse(
                "remote sends require a qualified target in the form `<agent>@<team>`".to_string(),
            )
            .with_recovery(
                "Provide the team in `atm send <agent>@<team> --host <host> ...` before retrying the remote send.",
            ));
        }
        return Ok(ParsedSendTarget {
            to: trimmed.parse()?,
            remote_host: None,
        });
    };

    validate_send_target_segment(raw_agent, "agent")?;

    if let Some(explicit_host) = explicit_host {
        if raw_team_or_remote.contains('.') {
            return Err(AtmError::address_parse(
                "cannot combine inline remote host syntax with `--host`".to_string(),
            )
            .with_recovery(
                "Use exactly one remote-target form: either `<agent>@<team>.<host>` or `<agent>@<team> --host <host>`.",
            ));
        }
        validate_send_target_segment(raw_team_or_remote, "team")?;
        return Ok(ParsedSendTarget {
            to: format!("{raw_agent}@{raw_team_or_remote}").parse()?,
            remote_host: Some(RemoteTargetHost::parse(explicit_host)?),
        });
    }

    if let Some((raw_team, raw_host)) = raw_team_or_remote.split_once('.') {
        validate_send_target_segment(raw_team, "team")?;
        return Ok(ParsedSendTarget {
            to: format!("{raw_agent}@{raw_team}").parse()?,
            remote_host: Some(RemoteTargetHost::parse(raw_host)?),
        });
    }

    validate_send_target_segment(raw_team_or_remote, "team")?;
    Ok(ParsedSendTarget {
        to: format!("{raw_agent}@{raw_team_or_remote}").parse()?,
        remote_host: None,
    })
}

fn validate_send_target_segment(value: &str, kind: &str) -> Result<(), AtmError> {
    crate::address::validate_path_segment(value, kind)?;
    if value.contains('.') {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain `.`"
        ))
        .with_recovery(
            "Use `-` or `_` in team/member names, and reserve `.` for the remote-host separator in `<agent>@<team>.<host>`.",
        ));
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub caller_identity: AgentName,
    pub caller_team: TeamName,
    pub to: AgentAddress,
    pub message_source: SendMessageSource,
    pub summary_override: Option<String>,
    pub requires_ack: bool,
    pub task_id: Option<TaskId>,
    pub parent_message_id: Option<AtmMessageId>,
    pub acknowledges_message_id: Option<AtmMessageId>,
    pub thread_mode: Option<ThreadMode>,
    pub expires_at: Option<crate::types::IsoTimestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_remote_host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_host: Option<RemoteTargetHost>,
    pub dry_run: bool,
}

impl SendRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        to: &str,
        caller_team: TeamName,
        message_source: SendMessageSource,
        summary_override: Option<String>,
        requires_ack: bool,
        task_id: Option<TaskId>,
        dry_run: bool,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            caller_identity,
            caller_team,
            to: to.parse()?,
            message_source,
            summary_override,
            requires_ack,
            task_id,
            parent_message_id: None,
            acknowledges_message_id: None,
            thread_mode: None,
            expires_at: None,
            source_remote_host: None,
            remote_host: None,
            dry_run,
        })
    }

    /// Build the canonical outbound message for `atm ack`.
    ///
    /// The acknowledgement command has no transport payload of its own: it is
    /// a [`SendRequest`] with `acknowledges_message_id` populated. The daemon
    /// resolves the original sender before the single routing decision.
    pub fn acknowledgement(
        home_dir: PathBuf,
        current_dir: PathBuf,
        caller_identity: AgentName,
        caller_team: TeamName,
        message_id: AtmMessageId,
        reply_body: String,
    ) -> Result<Self, AtmError> {
        let reply_text = input::validate_message_text(reply_body)?;
        let mut request = Self::new(
            home_dir,
            current_dir,
            caller_identity.clone(),
            &format!("{caller_identity}@{caller_team}"),
            caller_team,
            SendMessageSource::Inline(reply_text.clone()),
            Some(summary::build_summary(&reply_text, None)),
            false,
            None,
            false,
        )?;
        request.acknowledges_message_id = Some(message_id);
        Ok(request)
    }
}

/// Resolve an acknowledgement's recipient while retaining the sole outbound
/// payload shape. The daemon makes the single local-or-remote routing decision
/// after this preparation step.
pub fn resolve_acknowledgement_request(
    runtime: &LocalServiceRuntime,
    request: SendRequest,
) -> Result<SendRequest, AtmError> {
    let message_id = request.acknowledges_message_id.ok_or_else(|| {
        AtmError::validation("ack send is missing acknowledges_message_id".to_string())
    })?;
    let actor = request.caller_identity.clone();
    let team = request.caller_team.clone();
    let (source, _, has_successor) = load_acknowledgement_source(runtime, &request, message_id)?;
    if has_successor {
        return Err(AtmError::validation(format!(
            "message {message_id} has been updated; acknowledge the current terminal message instead"
        )));
    }
    if !matches!(
        crate::read::state::derive_ack_state(&source.envelope),
        crate::types::AckState::PendingAck
    ) {
        return Err(AtmError::validation(format!(
            "message {message_id} is not pending acknowledgement"
        )));
    }
    let (recipient, recipient_team, remote_host) =
        resolve_ack_recipient(runtime, &source.envelope, &actor, &team)?;
    // Keep acknowledgement requests on the canonical send shape.  The only
    // send-specific data that resolution changes is its destination and task
    // context; rebuilding a second request here would create an ack-only
    // payload path.
    let mut resolved = request;
    resolved.caller_identity = actor;
    resolved.caller_team = team;
    resolved.to = format!("{recipient}@{recipient_team}").parse()?;
    resolved.task_id = source.envelope.task_id.clone();
    resolved.remote_host = remote_host
        .map(|host| RemoteTargetHost::parse(&host))
        .transpose()?;
    Ok(resolved)
}

fn load_acknowledgement_source(
    runtime: &LocalServiceRuntime,
    request: &SendRequest,
    message_id: AtmMessageId,
) -> Result<(crate::boundary::Message, crate::boundary::MessageKey, bool), AtmError> {
    let rows = runtime.query_mailbox_metadata_rows(
        &request.home_dir,
        &request.caller_team,
        &request.caller_identity,
        None,
    )?;
    let row = rows
        .iter()
        .find(|row| row.message_id == Some(message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {message_id} was not found in {}@{}",
                request.caller_identity, request.caller_team
            ))
        })?;
    let has_successor = rows
        .iter()
        .any(|row| row.parent_message_id == Some(message_id));
    let source = runtime
        .load_message_record(
            &request.home_dir,
            &request.caller_team,
            &request.caller_identity,
            &row.message_key,
        )?
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {message_id} metadata could not be reloaded from sqlite"
            ))
        })?;
    Ok((source, row.message_key.clone(), has_successor))
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

/// Result of sending one ATM mailbox message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendOutcome {
    pub action: CommandAction,
    pub team: TeamName,
    pub agent: AgentName,
    pub sender: AgentName,
    pub outcome: SendCommandOutcome,
    pub message_id: AtmMessageId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_message_id: Option<AtmMessageId>,
    pub requires_ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<WarningEntry>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SendCommandOutcome {
    Sent,
    Deferred,
    DryRun,
}

impl SendCommandOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::Deferred => "deferred",
            Self::DryRun => "dry_run",
        }
    }
}

/// Send one mailbox message to a team member.
///
/// # Errors
///
/// Returns [`AtmError`] with
/// [`crate::error_codes::AtmErrorCode::IdentityUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamUnavailable`],
/// [`crate::error_codes::AtmErrorCode::TeamNotFound`],
/// [`crate::error_codes::AtmErrorCode::AgentNotFound`],
/// [`crate::error_codes::AtmErrorCode::AddressParseFailed`],
/// [`crate::error_codes::AtmErrorCode::SelfAddressedSendInvalid`],
/// [`crate::error_codes::AtmErrorCode::FilePolicyRejected`],
/// [`crate::error_codes::AtmErrorCode::MailboxReadFailed`], or
/// [`crate::error_codes::AtmErrorCode::MailboxWriteFailed`] when sender
/// identity cannot be resolved, recipient or team validation fails,
/// message/file-policy validation fails, or mailbox persistence fails.
pub fn send_mail(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError> {
    let runtime = default_runtime()?;
    send_mail_with_runtime(request, observability, &runtime)
}

pub fn send_mail_with_runtime(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
) -> Result<SendOutcome, AtmError> {
    send_mail_with_runtime_impl(request, observability, runtime, None)
}

pub fn send_mail_with_runtime_and_post_send_emitter(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    post_send_emitter: &dyn PostSendHookEmitter,
) -> Result<SendOutcome, AtmError> {
    send_mail_with_runtime_impl(request, observability, runtime, Some(post_send_emitter))
}

fn send_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    post_send_emitter: Option<&dyn PostSendHookEmitter>,
) -> Result<SendOutcome, AtmError> {
    let context = prepare_send_context(runtime, &request)?;
    let task_id = request.task_id.clone();
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &context.recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override.clone());
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    let persistence = persist_send_message(
        runtime,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        timestamp,
        requires_ack,
        task_id.clone(),
    )?;
    finalize_send_outcome(
        runtime,
        observability,
        post_send_emitter,
        &request,
        &context,
        &body,
        &summary,
        message_id,
        requires_ack,
        task_id,
        persistence,
    )
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[cfg(test)]
mod acknowledgement_request_tests {
    use super::{SendMessageSource, SendRequest};
    use crate::schema::AtmMessageId;
    use crate::types::{AgentName, TeamName};

    #[test]
    fn acknowledgement_is_a_canonical_send_request() {
        let message_id = AtmMessageId::new();
        let request = SendRequest::acknowledgement(
            std::path::PathBuf::from("/tmp/home"),
            std::path::PathBuf::from("/tmp/current"),
            AgentName::from_validated("sender-a"),
            TeamName::from_validated("test-team"),
            message_id,
            "received".to_string(),
        )
        .expect("canonical acknowledgement request");

        assert_eq!(request.acknowledges_message_id, Some(message_id));
        assert!(matches!(
            request.message_source,
            SendMessageSource::Inline(ref body) if body == "received"
        ));
    }
}

#[cfg(test)]
mod remote_target_parse_tests {
    use super::parse_send_target;

    #[test]
    fn remote_target_syntaxes_normalize_to_the_same_contract() {
        let inline = parse_send_target("qa-a@test-team.localhost", None).expect("inline target");
        let explicit =
            parse_send_target("qa-a@test-team", Some("localhost")).expect("explicit target");

        assert_eq!(inline.to, explicit.to);
        assert_eq!(inline.remote_host, explicit.remote_host);
        assert_eq!(inline.to.to_string(), "qa-a@test-team");
        assert_eq!(
            inline.remote_host.as_ref().expect("remote host").as_str(),
            "localhost"
        );
    }

    #[test]
    fn remote_target_rejects_combined_inline_and_explicit_host_forms() {
        let error = parse_send_target("qa-a@test-team.localhost", Some("127.0.0.1"))
            .expect_err("combined host forms must fail");
        assert!(
            error
                .message
                .contains("cannot combine inline remote host syntax")
        );
    }
}

#[cfg(test)]
mod graft_warning_tests;
#[cfg(test)]
mod tests;
