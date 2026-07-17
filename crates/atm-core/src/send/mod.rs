//! Send command service implementation and post-send hook handling.

use std::net::IpAddr;
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
pub struct PeerLoopbackHost(String);

impl PeerLoopbackHost {
    pub fn parse(value: &str) -> Result<Self, AtmError> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(AtmError::address_parse("loopback host must not be empty").with_recovery(
                "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
            ));
        }
        if trimmed.eq_ignore_ascii_case("localhost") || trimmed.parse::<IpAddr>().is_ok() {
            return Ok(Self(trimmed.to_string()));
        }
        Err(AtmError::address_parse(format!(
            "unsupported loopback host `{trimmed}`; expected `localhost` or a literal IP address"
        ))
        .with_recovery(
            "Use `loopback@localhost` or `loopback@<literal-ip>` before retrying the loopback send.",
        ))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn parse_cli_target(raw_target: &str) -> Option<Result<Self, AtmError>> {
        let (agent, host) = raw_target.trim().split_once('@')?;
        if !agent.eq_ignore_ascii_case("loopback") {
            return None;
        }
        Some(Self::parse(host))
    }
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
    pub peer_loopback_host: Option<PeerLoopbackHost>,
    #[serde(default)]
    pub peer_loopback_delivery: bool,
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
            peer_loopback_host: None,
            peer_loopback_delivery: false,
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
