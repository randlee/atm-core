//! Send command service implementation and post-send hook handling.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Map;
use tracing::warn;

use crate::address::AgentAddress;
use crate::boundary::GraftPostSendPort;
use crate::config;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::delivery_plan::{
    DeliveryPlan, delivery_plan_disposition, logical_messages_from_persistence,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::AtmError;
use crate::observability::{CommandEvent, ObservabilityPort, action_name, outcome_label};
use crate::schema::{AtmMessageId, InboxMessage, ThreadMode};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};

#[allow(
    dead_code,
    reason = "Z.6 removes the production send-path config gate; alert-state helpers remain test-covered until the follow-on cleanup deletes the obsolete file-backed alert seam."
)]
mod alert_state;
mod delivery_persistence;
pub(crate) mod file_policy;
pub(crate) mod hook;
mod hook_tmux;
pub(crate) mod input;
mod missing_config_notice;
mod persistence;
pub(crate) mod summary;

pub(crate) use delivery_persistence::{DeliveryPersistenceDisposition, DeliveryPersistenceResult};
pub(crate) use persistence::persist_message_and_seed_workflow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SendMessageSource {
    Inline(String),
    Stdin,
    File {
        path: PathBuf,
        message: Option<String>,
    },
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
    pub recovery: Option<String>,
}

impl WarningEntry {
    pub fn new(message: impl Into<String>, recovery: Option<impl Into<String>>) -> Self {
        Self {
            message: message.into(),
            recovery: recovery.map(Into::into),
        }
    }

    pub fn render(&self) -> String {
        match &self.recovery {
            Some(recovery) => format!("{} Recovery: {recovery}", self.message),
            None => self.message.clone(),
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
/// [`crate::error_codes::AtmErrorCode::MessageValidationFailed`],
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

pub fn send_mail_with_runtime_and_graft_port(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &LocalServiceRuntime,
    graft_port: &dyn GraftPostSendPort,
) -> Result<SendOutcome, AtmError> {
    send_mail_with_runtime_impl(request, observability, runtime, Some(graft_port))
}

fn send_mail_with_runtime_impl<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
    graft_port: Option<&dyn GraftPostSendPort>,
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
        graft_port,
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

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome pieces visible at the sprint seam."
)]
fn finalize_send_outcome<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
    graft_port: Option<&dyn GraftPostSendPort>,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    persistence: DeliveryPersistenceResult,
) -> Result<SendOutcome, AtmError> {
    let command_outcome = if request.dry_run {
        SendCommandOutcome::DryRun
    } else {
        SendCommandOutcome::Sent
    };
    let mut outcome = build_send_outcome(
        request,
        context,
        body,
        summary,
        message_id,
        requires_ack,
        task_id.clone(),
        command_outcome,
        &persistence,
    );
    if !request.dry_run {
        if let Some(warning) = build_claude_roster_warning(runtime, request, context)? {
            outcome.warnings.push(warning);
        }
        let plan = build_send_delivery_plan(context, requires_ack, &persistence)?;
        let execution = execute_delivery_plan(runtime, context.command_config.as_ref(), &plan)?;
        emit_delivery_plan_transitions(
            observability,
            DeliveryTransitionContext {
                family: context.delivery_family,
                team: &context.recipient.team,
                agent: &context.recipient.agent,
                sender: &context.canonical_sender,
                message_id,
                task_id: task_id.clone(),
            },
            &plan,
            &execution,
        )?;
        outcome.warnings.extend(execution.warnings);
        hook::emit_post_send_effects(
            &mut outcome.warnings,
            context.post_send_config.as_ref(),
            graft_port,
            &context.recipient,
            &context.delivery_snapshot,
            &plan.messages,
        );
    }
    emit_send_command_event(
        observability,
        command_outcome.as_str(),
        &outcome,
        task_id,
        &context.canonical_sender,
    );
    Ok(outcome)
}

fn build_claude_roster_warning<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    context: &SendExecutionContext,
) -> Result<Option<WarningEntry>, AtmError> {
    if !matches!(
        context.delivery_snapshot.harness,
        crate::delivery_policy::DeliveryHarnessPath::ClaudeCode
    ) {
        return Ok(None);
    }
    let team_dir = runtime.team_dir(&request.home_dir, &context.recipient.team)?;
    if !team_dir.join("config.json").is_file() {
        if !context.inbox_path.exists() {
            return Ok(None);
        }
        let mut warnings = Vec::new();
        missing_config_notice::warn_missing_team_config(
            runtime,
            request,
            &context.recipient,
            &team_dir,
            &context.inbox_path,
            &mut warnings,
        )?;
        return Ok(warnings.into_iter().next());
    }
    let roster = runtime.load_claude_code_team_roster(&context.recipient.team)?;
    if roster.contains_member(&context.recipient.agent) {
        return Ok(None);
    }
    Ok(Some(WarningEntry::new(
        format!(
            "'{}' is not on claude code roster {}/config.json",
            context.recipient.agent, context.recipient.team
        ),
        Some(
            "Import the member through the approved Claude config-ingress path or project ATM roster truth back into config.json before relying on Claude compatibility delivery.",
        ),
    )))
}

#[expect(
    clippy::too_many_arguments,
    reason = "Y.6 closeout keeps the explicit send outcome fields aligned with the command contract."
)]
fn build_send_outcome(
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    requires_ack: bool,
    task_id: Option<TaskId>,
    command_outcome: SendCommandOutcome,
    persistence: &DeliveryPersistenceResult,
) -> SendOutcome {
    let mut outcome = SendOutcome {
        action: CommandAction::Send,
        team: context.recipient.team.clone(),
        agent: context.recipient.agent.clone(),
        sender: context.canonical_sender.clone(),
        outcome: command_outcome,
        message_id,
        requires_ack,
        task_id,
        summary: Some(summary.to_string()),
        message: request.dry_run.then_some(body.to_string()),
        warnings: context.warnings.clone(),
        dry_run: request.dry_run,
    };
    outcome
        .warnings
        .extend(persistence.warnings.iter().cloned());
    outcome
}

fn build_send_delivery_plan(
    context: &SendExecutionContext,
    requires_ack: bool,
    persistence: &DeliveryPersistenceResult,
) -> Result<DeliveryPlan, AtmError> {
    Ok(DeliveryPlan::new(
        crate::delivery_plan::DeliveryPlanKind::Send,
        delivery_plan_disposition(persistence.disposition),
        crate::delivery_plan::delivery_target_for_snapshot(
            &context.inbox_path,
            &context.delivery_snapshot,
        ),
        context.recipient.clone(),
        logical_messages_from_persistence(persistence, requires_ack, false)
            .map_err(|error| {
                AtmError::mailbox_write(error.to_string()).with_recovery(
                    "Repair the persisted delivery record shape before retrying delivery-plan execution.",
                )
            })?,
        persistence.warnings.clone(),
    ))
}

struct SendExecutionContext {
    command_config: Option<config::AtmConfig>,
    post_send_config: Option<config::AtmConfig>,
    recipient: ResolvedRecipient,
    canonical_sender: AgentName,
    inbox_path: PathBuf,
    delivery_snapshot: DeliveryRecipientSnapshot,
    delivery_family: DeliveryEventFamily,
    warnings: Vec<WarningEntry>,
}

fn prepare_send_context<
    R: RetainedServiceRuntime + RetainedMailboxRuntime + crate::boundary::sealed::Sealed,
>(
    runtime: &R,
    request: &SendRequest,
) -> Result<SendExecutionContext, AtmError> {
    let command_config = runtime.load_config(&request.current_dir)?;
    let (post_send_config, warnings) = match hook::load_post_send_config_for_sender(
        runtime,
        &request.caller_team,
        &request.caller_identity,
    ) {
        Ok(config) => (config, Vec::new()),
        Err(error) => (
            None,
            vec![WarningEntry::new(
                format!(
                    "warning: post-send hook config lookup failed for {}@{}: {}.",
                    request.caller_identity, request.caller_team, error.message
                ),
                error.primary_recovery().map(str::to_owned),
            )],
        ),
    };
    let canonical_sender = request.caller_identity.clone();
    let recipient = resolve_recipient(&request.to, &request.caller_team, command_config.as_ref())?;
    let team_dir = runtime.team_dir(&request.home_dir, &recipient.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&recipient.team));
    }
    let inbox_path = runtime.inbox_path(&request.home_dir, &recipient.team, &recipient.agent)?;
    let delivery_policy = DeliveryPolicyCoordinator::new();
    let delivery_snapshot =
        delivery_policy.resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)?;
    let delivery_family = DeliveryPolicyCoordinator::resolve_send_family(
        request.parent_message_id,
        request.thread_mode,
    );
    Ok(SendExecutionContext {
        command_config,
        post_send_config,
        recipient,
        canonical_sender,
        inbox_path,
        delivery_snapshot,
        delivery_family,
        warnings,
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "Send persistence needs the explicit request/body/message envelope fields documented in the Y.4 state-machine seam."
)]
fn persist_send_message<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    context: &SendExecutionContext,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
    task_id: Option<TaskId>,
) -> Result<DeliveryPersistenceResult, AtmError> {
    if request.dry_run {
        return Ok(DeliveryPersistenceResult::persisted(InboxMessage {
            from: context.canonical_sender.clone(),
            text: body.to_string(),
            timestamp,
            read: false,
            source_team: Some(request.caller_team.clone()),
            summary: Some(summary.to_string()),
            message_id: Some(message_id),
            pending_ack_at: requires_ack.then_some(timestamp),
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: request.parent_message_id,
            thread_mode: request.thread_mode,
            expires_at: request.expires_at,
            task_id: task_id.clone(),
            extra: Map::new(),
        }));
    }
    let envelope = InboxMessage {
        from: context.canonical_sender.clone(),
        text: body.to_string(),
        timestamp,
        read: false,
        source_team: Some(request.caller_team.clone()),
        summary: Some(summary.to_string()),
        message_id: Some(message_id),
        pending_ack_at: requires_ack.then_some(timestamp),
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: request.parent_message_id,
        thread_mode: request.thread_mode,
        expires_at: request.expires_at,
        task_id: task_id.clone(),
        extra: Map::new(),
    };
    let persistence = persist_message_and_seed_workflow(
        runtime,
        &request.home_dir,
        &context.delivery_snapshot,
        &context.inbox_path,
        &envelope,
        false,
    )?;
    Ok(persistence)
}

fn emit_send_command_event(
    observability: &dyn ObservabilityPort,
    outcome_name: &'static str,
    outcome: &SendOutcome,
    task_id: Option<TaskId>,
    canonical_sender: &AgentName,
) {
    if let Err(error) = observability.emit(CommandEvent {
        command: "send",
        action: action_name("send"),
        outcome: outcome_label(outcome_name),
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: canonical_sender.clone(),
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        warn!(%error, command = "send", action = "send", "failed to emit send command event");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRecipient {
    pub(crate) agent: AgentName,
    pub(crate) team: TeamName,
}

fn resolve_recipient(
    target_address: &AgentAddress,
    caller_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    let team = target_address
        .team
        .as_deref()
        .and_then(|team| team.parse().ok())
        .or_else(|| Some(caller_team.clone()))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedRecipient {
        agent: AgentName::from_validated(config::aliases::resolve_agent(
            &target_address.agent,
            config,
        )),
        team,
    })
}

fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &TeamName,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::Inline(message) => input::validate_message_text(message.clone()),
        SendMessageSource::Stdin => input::read_message_from_stdin(),
        SendMessageSource::File { path, message } => {
            input::validate_message_text(file_policy::process_file_reference(
                path,
                message.as_deref(),
                team_name,
                current_dir,
                home_dir,
            )?)
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn prepare_threaded_message(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
) -> Result<(), AtmError> {
    match (
        envelope.parent_message_id,
        envelope.thread_mode,
        envelope.expires_at,
    ) {
        (None, None, _) => Ok(()),
        (Some(_), Some(_), Some(_)) => Err(AtmError::validation(
            "ephemeral messages may not participate in a message thread",
        )
        .with_recovery(
            "Send the message either as a standalone ephemeral note or as a non-ephemeral thread update.",
        )),
        (Some(parent_id), Some(_), None) => validate_thread_append(envelope, inbox_messages, parent_id),
        (Some(_), None, _) | (None, Some(_), _) => Err(AtmError::validation(
            "thread updates must set both parent_message_id and thread_mode",
        )
        .with_recovery(
            "Provide both the parent message id and either add-details or supersede when appending to an existing thread.",
        )),
    }
}

fn validate_thread_append(
    envelope: &mut InboxMessage,
    inbox_messages: &[InboxMessage],
    parent_id: AtmMessageId,
) -> Result<(), AtmError> {
    let index = ThreadIndex::new(inbox_messages);
    let parent = index.message(parent_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread parent message {} was not found in the recipient inbox",
            parent_id
        ))
        .with_recovery(
            "Refresh the recipient inbox state and retry the update against a message id that still exists in that thread.",
        )
    })?;

    if is_ephemeral(parent) {
        return Err(AtmError::validation(
            "ephemeral messages may not be updated or superseded",
        )
        .with_recovery(
            "Send a fresh standalone message instead of trying to append to an ephemeral message.",
        ));
    }

    let Some(root_id) = index.root_id(parent_id) else {
        return Err(AtmError::validation(format!(
            "thread root could not be resolved for parent message {}",
            parent_id
        ))
        .with_recovery(
            "Repair the malformed message thread or resend the correction as a fresh standalone message.",
        ));
    };
    let root = index.message(root_id).ok_or_else(|| {
        AtmError::validation(format!(
            "thread root message {} was not found in the recipient inbox",
            root_id
        ))
        .with_recovery(
            "Repair the malformed message thread or resend the correction as a fresh standalone message.",
        )
    })?;

    if canonical_sender_identity(root) != canonical_sender_identity(envelope) {
        return Err(AtmError::validation(
            "only the original sender may append details or supersede a message thread",
        )
        .with_recovery(
            "Send a new message instead of appending to a thread you did not originate.",
        ));
    }

    if index.has_successor(parent_id) {
        return Err(AtmError::validation(format!(
            "message {} already has a successor; ATM threads are strictly linear",
            parent_id
        ))
        .with_recovery(
            "Append to the current terminal message in the thread instead of branching from an older message.",
        ));
    }

    let thread_requires_ack = index.thread_requires_ack(parent_id);
    envelope.pending_ack_at = thread_requires_ack.then_some(envelope.timestamp);
    envelope.acknowledged_at = None;
    Ok(())
}

#[allow(
    dead_code,
    reason = "Retained for the dormant direct post-send hook helper."
)]
pub(super) fn qualified_sender_identity(
    sender: &AgentName,
    sender_team: Option<&TeamName>,
) -> String {
    sender_team
        .map(|team| format!("{sender}@{team}"))
        .unwrap_or_else(|| sender.to_string())
}

#[cfg(test)]
mod tests;
