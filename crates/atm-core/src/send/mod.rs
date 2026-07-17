//! Send command service implementation and post-send hook handling.

use std::path::PathBuf;
use std::time::Duration;

use crate::address::AgentAddress;
use crate::boundary::PostSendHookEmitter;
use crate::config;
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::AtmError;
use crate::error_codes::AtmErrorCode;
use crate::observability::ObservabilityPort;
use crate::schema::{AtmMessageId, ThreadMode};
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
pub(crate) mod summary;
mod target;
mod threading_helpers;

use context::{SendExecutionContext, persist_send_message, prepare_send_context};
pub(crate) use delivery_persistence::{DeliveryPersistenceDisposition, DeliveryPersistenceResult};
use outcome::finalize_send_outcome;
pub(crate) use persistence::persist_message;
pub(crate) use target::{
    ResolvedRecipient, qualified_sender_identity, resolve_message_body, resolve_recipient,
    validate_non_self_recipient,
};
pub(crate) use threading_helpers::prepare_threaded_message;

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

    if let Some((raw_team, raw_host)) = raw_team_or_remote.rsplit_once('.') {
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
    pub thread_mode: Option<ThreadMode>,
    pub expires_at: Option<crate::types::IsoTimestamp>,
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
            thread_mode: None,
            expires_at: None,
            remote_host: None,
            dry_run,
        })
    }
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
    DryRun,
}

impl SendCommandOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sent => "sent",
            Self::DryRun => "dry_run",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WarningEntry {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<AtmErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<String>,
}

impl WarningEntry {
    pub fn new(message: impl Into<String>, recovery: Option<impl Into<String>>) -> Self {
        Self {
            message: message.into(),
            code: None,
            recovery: recovery.map(Into::into),
        }
    }

    pub fn with_code(
        code: AtmErrorCode,
        message: impl Into<String>,
        recovery: Option<impl Into<String>>,
    ) -> Self {
        Self {
            message: message.into(),
            code: Some(code),
            recovery: recovery.map(Into::into),
        }
    }

    pub fn render(&self) -> String {
        let message = match self.code {
            Some(code) if !self.message.contains(code.as_str()) => {
                format!("{} [{}]", self.message, code.as_str())
            }
            _ => self.message.clone(),
        };
        match &self.recovery {
            Some(recovery) => format!("{message} Recovery: {recovery}"),
            None => message,
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
mod graft_warning_tests;
#[cfg(test)]
mod tests;
