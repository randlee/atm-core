//! Send command service implementation and post-send hook handling.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Map;
use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::delivery_execution::{
    DeliveryTransitionContext, emit_delivery_plan_transitions, execute_delivery_plan,
};
use crate::delivery_plan::{
    DeliveryPlan, DeliveryPlanDisposition, LogicalMessage, delivery_target_for_snapshot,
};
use crate::delivery_policy::{
    DeliveryEventFamily, DeliveryPolicyCoordinator, DeliveryRecipientSnapshot,
};
use crate::error::{AtmError, AtmErrorCode};
use crate::identity;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};

mod alert_state;
mod delivery_persistence;
pub(crate) mod file_policy;
pub(super) mod hook;
pub(crate) mod input;
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
    pub sender_override: Option<AgentName>,
    pub to: AgentAddress,
    pub team_override: Option<TeamName>,
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
        sender_override: Option<&str>,
        to: &str,
        team_override: Option<&str>,
        message_source: SendMessageSource,
        summary_override: Option<String>,
        requires_ack: bool,
        task_id: Option<TaskId>,
        dry_run: bool,
    ) -> Result<Self, AtmError> {
        Ok(Self {
            home_dir,
            current_dir,
            sender_override: sender_override.map(str::parse).transpose()?,
            to: to.parse()?,
            team_override: team_override.map(str::parse).transpose()?,
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
    send_mail_with_runtime_impl(request, observability, runtime)
}

fn send_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
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
fn finalize_send_outcome<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    observability: &dyn ObservabilityPort,
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
        let plan = build_send_delivery_plan(context, requires_ack, &persistence)?;
        let execution = execute_delivery_plan(runtime, context.config.as_ref(), &plan)?;
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
    let mut messages = vec![
        LogicalMessage::new(persistence.original_message.clone(), requires_ack, false)
            .map_err(AtmError::mailbox_write)?,
    ];
    if let Some(companion_message) = persistence.companion_message.clone() {
        messages.push(
            LogicalMessage::new(companion_message, false, false)
                .map_err(AtmError::mailbox_write)?,
        );
    }
    Ok(DeliveryPlan::new(
        match persistence.disposition {
            DeliveryPersistenceDisposition::Persisted => DeliveryPlanDisposition::Persisted,
            DeliveryPersistenceDisposition::SqliteFailedRecovered => {
                DeliveryPlanDisposition::SqliteFailedRecovered
            }
        },
        delivery_target_for_snapshot(&context.inbox_path, &context.delivery_snapshot),
        context.recipient.clone(),
        context.delivery_snapshot.recipient_pane_id.clone(),
        messages,
        persistence.warnings.clone(),
    ))
}

struct SendExecutionContext {
    config: Option<config::AtmConfig>,
    recipient: ResolvedRecipient,
    sender_team: Option<TeamName>,
    canonical_sender: AgentName,
    display_sender: AgentName,
    inbox_path: PathBuf,
    delivery_snapshot: DeliveryRecipientSnapshot,
    delivery_family: DeliveryEventFamily,
    warnings: Vec<WarningEntry>,
}

fn prepare_send_context<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
) -> Result<SendExecutionContext, AtmError> {
    let config = runtime.load_config(&request.current_dir)?;
    let canonical_sender =
        identity::resolve_sender_identity(request.sender_override.as_deref(), config.as_ref())?;
    let recipient = resolve_recipient(
        &request.to,
        request.team_override.as_deref(),
        config.as_ref(),
    )?;
    let sender_team = config::resolve_team(None, config.as_ref());
    let display_sender = display_sender_identity(
        &canonical_sender,
        request.sender_override.as_ref(),
        sender_team.as_ref(),
        &recipient.team,
        config.as_ref(),
    );
    let team_dir = runtime.team_dir(&request.home_dir, &recipient.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&recipient.team));
    }
    let inbox_path = runtime.inbox_path(&request.home_dir, &recipient.team, &recipient.agent)?;
    let mut warnings = Vec::new();
    validate_send_target(
        runtime,
        request,
        &recipient,
        &team_dir,
        &inbox_path,
        &mut warnings,
    )?;
    let delivery_policy = DeliveryPolicyCoordinator::new();
    let delivery_snapshot =
        delivery_policy.resolve_recipient_snapshot(runtime, &recipient.team, &recipient.agent)?;
    let delivery_family = DeliveryPolicyCoordinator::resolve_send_family(
        request.parent_message_id,
        request.thread_mode,
    );
    Ok(SendExecutionContext {
        config,
        recipient,
        sender_team,
        canonical_sender,
        display_sender,
        inbox_path,
        delivery_snapshot,
        delivery_family,
        warnings,
    })
}

fn validate_send_target<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    recipient: &ResolvedRecipient,
    team_dir: &Path,
    inbox_path: &Path,
    warnings: &mut Vec<WarningEntry>,
) -> Result<(), AtmError> {
    match runtime.load_team_config(team_dir) {
        Ok(team_config) => {
            clear_missing_team_config_alert(&request.home_dir, team_dir);
            if !team_config
                .members
                .iter()
                .any(|member| member.name == recipient.agent.as_str())
            {
                return Err(AtmError::agent_not_found(&recipient.agent, &recipient.team));
            }
            Ok(())
        }
        Err(error) if error.is_missing_document() => {
            warn_missing_team_config(runtime, request, recipient, team_dir, inbox_path, warnings)
        }
        Err(error) => Err(error),
    }
}

fn clear_missing_team_config_alert(home_dir: &Path, team_dir: &Path) {
    alert_state::clear_missing_team_config_alert(
        home_dir,
        &alert_state::missing_team_config_alert_key(team_dir),
    );
}

fn warn_missing_team_config<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    recipient: &ResolvedRecipient,
    team_dir: &Path,
    inbox_path: &Path,
    warnings: &mut Vec<WarningEntry>,
) -> Result<(), AtmError> {
    if !inbox_path.exists() {
        return Err(build_missing_config_error(team_dir, inbox_path));
    }
    warnings.push(missing_config_warning(recipient, team_dir));
    warn!(
        code = %AtmErrorCode::WarningMissingTeamConfigFallback,
        config_path = %team_dir.join("config.json").display(),
        recipient = %recipient.agent,
        team = %recipient.team,
        "send used existing inbox fallback; team config is missing"
    );
    if !request.dry_run {
        notify_team_lead_missing_config(
            runtime,
            &request.home_dir,
            team_dir,
            &recipient.team,
            &recipient.agent,
        );
    }
    Ok(())
}

fn build_missing_config_error(team_dir: &Path, inbox_path: &Path) -> AtmError {
    AtmError::missing_document(format!(
        "team config is missing at {} and inbox {} does not exist, so send cannot safely proceed",
        team_dir.join("config.json").display(),
        inbox_path.display()
    ))
    .with_recovery(
        "Restore config.json for the team or create the intended inbox by an approved workflow before retrying.",
    )
}

fn missing_config_warning(recipient: &ResolvedRecipient, team_dir: &Path) -> WarningEntry {
    WarningEntry::new(
        format!(
            "warning: team config is missing at {}; send used existing inbox fallback for {}@{}.",
            team_dir.join("config.json").display(),
            recipient.agent,
            recipient.team
        ),
        Some("Restore the team config."),
    )
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
        return Ok(DeliveryPersistenceResult::persisted(MessageEnvelope {
            from: context.display_sender.clone(),
            text: body.to_string(),
            timestamp,
            read: false,
            source_team: context
                .sender_team
                .clone()
                .or_else(|| Some(context.recipient.team.clone())),
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
    let envelope = MessageEnvelope {
        from: context.display_sender.clone(),
        text: body.to_string(),
        timestamp,
        read: false,
        source_team: context
            .sender_team
            .clone()
            .or_else(|| Some(context.recipient.team.clone())),
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
        action: "send",
        outcome: outcome_name,
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

#[derive(Clone, Copy)]
pub(crate) struct PostSendHookContext<'a> {
    pub(crate) sender: &'a AgentName,
    pub(crate) sender_team: Option<&'a TeamName>,
    pub(crate) recipient: &'a ResolvedRecipient,
    pub(crate) recipient_pane_id: Option<&'a str>,
    pub(crate) message_id: AtmMessageId,
    pub(crate) requires_ack: bool,
    pub(crate) is_ack: bool,
    pub(crate) task_id: Option<&'a TaskId>,
}

fn resolve_recipient(
    target_address: &AgentAddress,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    let team = target_address
        .team
        .as_deref()
        .and_then(|team| team.parse().ok())
        .or_else(|| config::resolve_team(team_override, config))
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

fn notify_team_lead_missing_config(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    team_dir: &Path,
    team: &TeamName,
    recipient: &AgentName,
) {
    // Accepted risk: this fallback notice is best-effort only. ATM may race a fast
    // shutdown and skip persistence rather than threading a shutdown token through
    // this compatibility-only warning path.
    let alert_key = alert_state::missing_team_config_alert_key(team_dir);
    if !alert_state::register_missing_team_config_alert(home_dir, &alert_key) {
        return;
    }

    let team_lead_agent = AgentName::from_validated(ROLE_TEAM_LEAD);
    let team_lead_inbox = match load_team_lead_inbox_path(runtime, home_dir, team, &team_lead_agent)
    {
        Ok(path) => path,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                %error,
                team = %team,
                "failed to resolve reserved missing-config inbox for notice"
            );
            return;
        }
    };

    let config_path = team_dir.join("config.json");
    let timestamp = IsoTimestamp::now();
    let notice = build_missing_config_notice(team, recipient, &config_path, timestamp);
    let snapshot = match resolve_team_lead_snapshot(runtime, team) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                %error,
                team = %team,
                "failed to resolve reserved missing-config delivery snapshot"
            );
            return;
        }
    };
    if let Err(error) =
        persist_missing_config_notice(runtime, home_dir, &snapshot, &team_lead_inbox, &notice)
    {
        warn!(
            code = %AtmErrorCode::WarningMissingTeamConfigFallback,
            %error,
            path = %team_lead_inbox.display(),
            team = %team,
            "failed to persist missing-config notice via shared mailbox/workflow commit path"
        );
    }
}

fn load_team_lead_inbox_path(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    team: &TeamName,
    team_lead_agent: &AgentName,
) -> Result<PathBuf, AtmError> {
    runtime.inbox_path(home_dir, team, team_lead_agent)
}

fn build_missing_config_notice(
    team: &TeamName,
    recipient: &AgentName,
    config_path: &Path,
    timestamp: IsoTimestamp,
) -> MessageEnvelope {
    MessageEnvelope {
        from: AgentName::from_validated("atm-identity-missing"),
        text: format!(
            "ATM warning: send used existing inbox fallback for {recipient}@{team} because team config is missing at {}. Please restore config.json.",
            config_path.display()
        ),
        timestamp,
        read: false,
        source_team: Some(team.clone()),
        summary: Some(format!(
            "ATM warning: missing team config fallback used for {recipient}@{team}"
        )),
        message_id: Some(AtmMessageId::new()),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: None,
        thread_mode: None,
        expires_at: None,
        task_id: None,
        extra: Map::new(),
    }
}

fn resolve_team_lead_snapshot(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    team: &TeamName,
) -> Result<DeliveryRecipientSnapshot, AtmError> {
    DeliveryPolicyCoordinator::new().resolve_recipient_snapshot(
        runtime,
        team,
        &AgentName::from_validated(ROLE_TEAM_LEAD),
    )
}

fn persist_missing_config_notice(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    snapshot: &DeliveryRecipientSnapshot,
    team_lead_inbox: &Path,
    notice: &MessageEnvelope,
) -> Result<(), AtmError> {
    let persistence = persist_message_and_seed_workflow(
        runtime,
        home_dir,
        snapshot,
        team_lead_inbox,
        notice,
        true,
    )?;
    let mut messages = vec![
        LogicalMessage::new(persistence.original_message, false, false)
            .map_err(AtmError::mailbox_write)?,
    ];
    if let Some(companion_message) = persistence.companion_message {
        messages.push(
            LogicalMessage::new(companion_message, false, false)
                .map_err(AtmError::mailbox_write)?,
        );
    }
    let plan = DeliveryPlan::new(
        match persistence.disposition {
            DeliveryPersistenceDisposition::Persisted => DeliveryPlanDisposition::Persisted,
            DeliveryPersistenceDisposition::SqliteFailedRecovered => {
                DeliveryPlanDisposition::SqliteFailedRecovered
            }
        },
        delivery_target_for_snapshot(team_lead_inbox, snapshot),
        ResolvedRecipient {
            agent: AgentName::from_validated(ROLE_TEAM_LEAD),
            team: snapshot.team.clone(),
        },
        snapshot.recipient_pane_id.clone(),
        messages,
        persistence.warnings,
    );
    let execution = execute_delivery_plan(runtime, None, &plan)?;
    for warning in plan.warnings {
        warn!(message = %warning.message, "compatibility append degraded for missing-config notice");
    }
    for warning in execution.warnings {
        warn!(message = %warning.message, "compatibility append degraded for missing-config notice");
    }
    Ok(())
}

fn prepare_threaded_message(
    envelope: &mut MessageEnvelope,
    inbox_messages: &[MessageEnvelope],
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
    envelope: &mut MessageEnvelope,
    inbox_messages: &[MessageEnvelope],
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

fn display_sender_identity(
    canonical_sender: &AgentName,
    sender_override: Option<&AgentName>,
    sender_team: Option<&TeamName>,
    recipient_team: &TeamName,
    config: Option<&config::AtmConfig>,
) -> AgentName {
    let cross_team = sender_team.is_some_and(|team| team != recipient_team);
    if !cross_team {
        return canonical_sender.clone();
    }

    if let Some(sender_override) = sender_override
        && config::aliases::resolve_agent(sender_override, config) == canonical_sender.as_str()
    {
        return sender_override.clone();
    }

    config::aliases::preferred_alias(canonical_sender.as_str(), config)
        .map(AgentName::from_validated)
        .unwrap_or_else(|| canonical_sender.clone())
}

pub(super) fn qualified_sender_identity(
    sender: &AgentName,
    sender_team: Option<&TeamName>,
) -> String {
    sender_team
        .map(|team| format!("{sender}@{team}"))
        .unwrap_or_else(|| sender.to_string())
}

pub(crate) fn maybe_run_post_send_hook(
    warnings: &mut Vec<WarningEntry>,
    config: Option<&config::AtmConfig>,
    context: PostSendHookContext<'_>,
) {
    hook::maybe_run_post_send_hook(warnings, config, context);
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::Map;
    use std::fs;
    use tempfile::tempdir;

    use super::{
        DeliveryPersistenceDisposition, PostSendHookContext, ResolvedRecipient,
        SendExecutionContext, WarningEntry, alert_state, build_send_delivery_plan,
        persist_message_and_seed_workflow, prepare_threaded_message,
    };
    use crate::boundary::{
        MailMessageState, MailStoreMailboxMetadataRow, MailStoreMessageRecord, MessageKey,
        NonClaudeOutboundDeliveryRequest, RosterHarness, RosterMemberKind, RosterMemberRecord,
    };
    use crate::config::AtmConfig;
    use crate::delivery_execution::{DeliveryExecutionDisposition, execute_delivery_plan};
    use crate::delivery_policy::{
        DeliveryEventFamily, DeliveryHarnessPath, DeliveryRecipientSnapshot,
    };
    use crate::error::AtmError;
    use crate::observability::{
        AtmLogQuery, AtmLogSnapshot, AtmObservabilityHealth, AtmObservabilityHealthState,
        CommandEvent, LogTailSession, ObservabilityPort,
    };
    use crate::process::process_is_alive;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AgentMember, TeamConfig};
    use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
    use crate::send::{SendCommandOutcome, SendMessageSource, SendRequest};
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TaskId, TeamName};
    use crate::workflow::WorkflowStateFile;

    fn message(
        from: &str,
        message_id: AtmMessageId,
        parent_message_id: Option<AtmMessageId>,
        thread_mode: Option<ThreadMode>,
    ) -> MessageEnvelope {
        MessageEnvelope {
            from: from.parse::<AgentName>().expect("agent"),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TEST_TEAM.parse::<TeamName>().expect("team")),
            summary: None,
            message_id: Some(message_id),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id,
            thread_mode,
            expires_at: None,
            task_id: None,
            extra: Map::new(),
        }
    }

    #[derive(Debug)]
    struct HookCapture {
        sender: AgentName,
        sender_team: Option<TeamName>,
        recipient: ResolvedRecipient,
        message_id: AtmMessageId,
        requires_ack: bool,
        is_ack: bool,
        task_id: Option<TaskId>,
    }

    // Mutex required: TestRuntime is shared via Arc across threads in concurrent send tests.
    struct TestRuntime {
        commit_error_message: Option<&'static str>,
        append_error_message: Option<&'static str>,
        recipient_harness: DeliveryHarnessPath,
        appended_messages: Mutex<Vec<MessageEnvelope>>,
        non_claude_deliveries: Mutex<Vec<NonClaudeOutboundDeliveryRequest>>,
        hook_captures: Mutex<Vec<HookCapture>>,
    }

    impl TestRuntime {
        fn new(
            commit_error_message: Option<&'static str>,
            append_error_message: Option<&'static str>,
            recipient_harness: DeliveryHarnessPath,
        ) -> Self {
            Self {
                commit_error_message,
                append_error_message,
                recipient_harness,
                appended_messages: Mutex::new(Vec::new()),
                non_claude_deliveries: Mutex::new(Vec::new()),
                hook_captures: Mutex::new(Vec::new()),
            }
        }
    }

    impl RetainedServiceRuntime for TestRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_team_config(&self, _team_dir: &Path) -> Result<TeamConfig, AtmError> {
            Ok(TeamConfig {
                members: vec![AgentMember::with_name(AgentName::from_validated(
                    "recipient",
                ))],
                extra: Map::new(),
            })
        }

        fn team_dir(&self, home_dir: &Path, _team: &TeamName) -> Result<PathBuf, AtmError> {
            Ok(home_dir.to_path_buf())
        }

        fn inbox_path(
            &self,
            home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            Ok(home_dir.join("inbox.jsonl"))
        }

        fn load_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<Option<IsoTimestamp>, AtmError> {
            Ok(None)
        }

        fn save_seen_watermark(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _timestamp: IsoTimestamp,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn mailbox_timeout_policy(&self) -> RetainedMailboxTimeoutPolicy {
            RetainedMailboxTimeoutPolicy {
                workflow_lock_timeout: Duration::from_millis(1),
            }
        }

        fn maybe_run_post_send_hook(
            &self,
            _warnings: &mut Vec<super::WarningEntry>,
            _config: Option<&AtmConfig>,
            context: PostSendHookContext<'_>,
        ) {
            self.hook_captures
                .lock()
                .expect("hook captures lock")
                .push(HookCapture {
                    sender: context.sender.clone(),
                    sender_team: context.sender_team.cloned(),
                    recipient: ResolvedRecipient {
                        agent: context.recipient.agent.clone(),
                        team: context.recipient.team.clone(),
                    },
                    message_id: context.message_id,
                    requires_ack: context.requires_ack,
                    is_ack: context.is_ack,
                    task_id: context.task_id.cloned(),
                });
        }

        fn rebuild_compat_inbox_projection(
            &self,
            _inbox_path: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), AtmError> {
            Ok(())
        }

        fn append_compat_inbox_message(
            &self,
            _inbox_path: &Path,
            message: &MessageEnvelope,
        ) -> Result<(), AtmError> {
            if let Some(message) = self.append_error_message {
                return Err(AtmError::mailbox_write(message));
            }
            self.appended_messages
                .lock()
                .expect("append captures lock")
                .push(message.clone());
            Ok(())
        }

        fn deliver_non_claude_payloads(
            &self,
            recipient: &DeliveryRecipientSnapshot,
            messages: &[MessageEnvelope],
        ) -> Result<(), AtmError> {
            self.non_claude_deliveries
                .lock()
                .expect("non-claude deliveries lock")
                .push(NonClaudeOutboundDeliveryRequest {
                    team: recipient.team.clone(),
                    agent: recipient.agent.clone(),
                    recipient_pane_id: recipient.recipient_pane_id.clone(),
                    messages: messages.to_vec(),
                });
            Ok(())
        }

        fn load_roster_member(
            &self,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<Option<crate::boundary::RosterMemberRecord>, AtmError> {
            Ok(Some(RosterMemberRecord {
                team_name: team.clone(),
                agent_name: agent.clone(),
                member_kind: RosterMemberKind::Permanent,
                harness: match self.recipient_harness {
                    DeliveryHarnessPath::ClaudeCode => RosterHarness::ClaudeCode,
                    DeliveryHarnessPath::NonClaude => RosterHarness::CodexCli,
                },
                agent_type: String::new(),
                model: String::new(),
                recipient_pane_id: None,
                metadata_json: Map::new(),
            }))
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            body: F,
        ) -> Result<T, AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
        {
            if let Some(message) = self.commit_error_message {
                return Err(AtmError::mailbox_write(message));
            }
            let mut workflow = WorkflowStateFile::default();
            body(&mut workflow).map(|(value, _dirty)| value)
        }
    }

    impl RetainedMailboxRuntime for TestRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<MailStoreMailboxMetadataRow>, AtmError> {
            Ok(Vec::new())
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &MessageKey,
        ) -> Result<Option<MailStoreMessageRecord>, AtmError> {
            Ok(None)
        }

        fn persist_message_record(&self, _record: MailStoreMessageRecord) -> Result<(), AtmError> {
            Ok(())
        }

        fn persist_message_state(&self, _state: MailMessageState) -> Result<(), AtmError> {
            Ok(())
        }
    }

    fn delivery_snapshot(harness: DeliveryHarnessPath) -> DeliveryRecipientSnapshot {
        DeliveryRecipientSnapshot {
            agent: AgentName::from_validated("recipient"),
            team: TeamName::from_validated(TEST_TEAM),
            harness,
            recipient_pane_id: None,
            roster_backed: true,
        }
    }

    fn outbound_message() -> MessageEnvelope {
        MessageEnvelope {
            from: AgentName::from_validated(TEST_SENDER),
            text: "hello".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TeamName::from_validated(TEST_TEAM)),
            summary: Some("hello".to_string()),
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: Some("task-123".parse().expect("task id")),
            extra: Map::new(),
        }
    }

    fn send_request(home_dir: &Path) -> SendRequest {
        SendRequest {
            home_dir: home_dir.to_path_buf(),
            current_dir: home_dir.to_path_buf(),
            sender_override: Some(AgentName::from_validated(TEST_SENDER)),
            to: format!("recipient@{TEST_TEAM}").parse().expect("address"),
            team_override: None,
            message_source: SendMessageSource::Inline("hello".to_string()),
            summary_override: Some("hello".to_string()),
            requires_ack: false,
            task_id: Some("task-123".parse().expect("task id")),
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            dry_run: false,
        }
    }

    #[derive(Default)]
    struct RecordingObservability {
        events: Mutex<Vec<CommandEvent>>,
    }

    impl crate::boundary::sealed::Sealed for RecordingObservability {}

    impl ObservabilityPort for RecordingObservability {
        fn emit(&self, event: CommandEvent) -> Result<(), AtmError> {
            self.events.lock().expect("events lock").push(event);
            Ok(())
        }

        fn query(&self, _req: AtmLogQuery) -> Result<AtmLogSnapshot, AtmError> {
            Ok(AtmLogSnapshot::default())
        }

        fn follow(&self, _req: AtmLogQuery) -> Result<LogTailSession, AtmError> {
            Ok(LogTailSession::empty())
        }

        fn health(&self) -> Result<AtmObservabilityHealth, AtmError> {
            Ok(AtmObservabilityHealth {
                active_log_path: None,
                logging_state: AtmObservabilityHealthState::Unavailable,
                query_state: Some(AtmObservabilityHealthState::Unavailable),
                detail: Some("test observer".to_string()),
            })
        }
    }

    #[test]
    fn load_send_alert_state_parse_errors_are_config_errors() {
        let tempdir = tempdir().expect("tempdir");
        let path = alert_state::state_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("state dir");
        }
        fs::write(&path, "{not-json").expect("state file");

        let error = alert_state::load(&path).expect_err("malformed state");
        assert!(error.is_config());
    }

    #[test]
    fn sqlite_failure_for_claude_preserves_original_and_companion_error_payloads() {
        let runtime = TestRuntime::new(
            Some("sqlite write failed"),
            None,
            DeliveryHarnessPath::ClaudeCode,
        );
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.jsonl");

        let result = persist_message_and_seed_workflow(
            &runtime,
            tempdir.path(),
            &delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
            &inbox_path,
            &outbound_message(),
            false,
        )
        .expect("sqlite fallback recovery");

        assert_eq!(
            result.disposition,
            DeliveryPersistenceDisposition::SqliteFailedRecovered
        );
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(result.original_message.from.as_str(), TEST_SENDER);
        assert_eq!(
            result
                .companion_message
                .as_ref()
                .expect("companion")
                .from
                .as_str(),
            "atm-system"
        );
    }

    #[test]
    fn sqlite_failure_for_non_claude_preserves_original_and_companion_payloads() {
        let runtime = TestRuntime::new(
            Some("sqlite write failed"),
            None,
            DeliveryHarnessPath::NonClaude,
        );
        let tempdir = tempdir().expect("tempdir");
        let inbox_path = tempdir.path().join("recipient.jsonl");

        let result = persist_message_and_seed_workflow(
            &runtime,
            tempdir.path(),
            &delivery_snapshot(DeliveryHarnessPath::NonClaude),
            &inbox_path,
            &outbound_message(),
            false,
        )
        .expect("sqlite fallback recovery");

        assert_eq!(
            result.disposition,
            DeliveryPersistenceDisposition::SqliteFailedRecovered
        );
        assert_eq!(result.original_message.from.as_str(), TEST_SENDER);
        assert_eq!(
            result
                .companion_message
                .as_ref()
                .expect("companion")
                .from
                .as_str(),
            "atm-system"
        );
    }

    #[test]
    fn append_failure_after_sqlite_commit_is_execution_only() {
        let runtime =
            TestRuntime::new(None, Some("append failed"), DeliveryHarnessPath::ClaudeCode);
        let tempdir = tempdir().expect("tempdir");
        let context = SendExecutionContext {
            config: None,
            recipient: ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            sender_team: Some(TeamName::from_validated(TEST_TEAM)),
            canonical_sender: AgentName::from_validated(TEST_SENDER),
            display_sender: AgentName::from_validated(TEST_SENDER),
            inbox_path: tempdir.path().join("recipient.jsonl"),
            delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
            delivery_family: DeliveryEventFamily::NewMessage,
            warnings: Vec::new(),
        };
        let persistence = crate::send::DeliveryPersistenceResult::persisted(outbound_message());
        let plan = build_send_delivery_plan(&context, false, &persistence).expect("plan");
        let execution =
            execute_delivery_plan(&runtime, None, &plan).expect("append degraded execution");

        assert_eq!(
            execution.disposition,
            DeliveryExecutionDisposition::AppendDegraded
        );
        assert_eq!(execution.warnings.len(), 1);
    }

    #[test]
    fn named_plan_builder_proves_payload_equality_across_harnesses() {
        let tempdir = tempdir().expect("tempdir");
        let original = outbound_message();
        let companion = MessageEnvelope {
            from: AgentName::from_validated("atm-system"),
            text: "sqlite failed".to_string(),
            timestamp: IsoTimestamp::now(),
            read: false,
            source_team: Some(TeamName::from_validated(TEST_TEAM)),
            summary: Some("sqlite failed".to_string()),
            message_id: Some(AtmMessageId::new()),
            pending_ack_at: None,
            acknowledged_at: None,
            acknowledges_message_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            task_id: original.task_id.clone(),
            extra: Map::new(),
        };
        let persistence = crate::send::DeliveryPersistenceResult::sqlite_failed_recovered(
            original.clone(),
            companion.clone(),
            WarningEntry::new("sqlite failed", Some("repair sqlite")),
        );
        let base_context = SendExecutionContext {
            config: None,
            recipient: ResolvedRecipient {
                agent: AgentName::from_validated("recipient"),
                team: TeamName::from_validated(TEST_TEAM),
            },
            sender_team: Some(TeamName::from_validated(TEST_TEAM)),
            canonical_sender: AgentName::from_validated(TEST_SENDER),
            display_sender: AgentName::from_validated(TEST_SENDER),
            inbox_path: tempdir.path().join("recipient.jsonl"),
            delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::ClaudeCode),
            delivery_family: DeliveryEventFamily::NewMessage,
            warnings: Vec::new(),
        };
        let claude_plan =
            build_send_delivery_plan(&base_context, false, &persistence).expect("claude plan");
        let non_claude_context = SendExecutionContext {
            delivery_snapshot: delivery_snapshot(DeliveryHarnessPath::NonClaude),
            ..base_context
        };
        let non_claude_plan = build_send_delivery_plan(&non_claude_context, false, &persistence)
            .expect("non-claude plan");

        assert_eq!(claude_plan.messages, non_claude_plan.messages);
        assert!(matches!(
            claude_plan.delivery_target,
            crate::delivery_plan::DeliveryTarget::ClaudeCode { .. }
        ));
        assert!(matches!(
            non_claude_plan.delivery_target,
            crate::delivery_plan::DeliveryTarget::NonClaude { .. }
        ));
    }

    #[test]
    fn named_companion_error_failure_handling_adds_explicit_warning() {
        let runtime = TestRuntime::new(
            Some("sqlite write failed"),
            Some("append failed"),
            DeliveryHarnessPath::ClaudeCode,
        );
        let observability = RecordingObservability::default();
        let tempdir = tempdir().expect("tempdir");

        let outcome = super::send_mail_with_runtime_impl(
            send_request(tempdir.path()),
            &observability,
            &runtime,
        )
        .expect("send outcome");

        assert!(outcome.warnings.iter().any(|warning| {
            warning
                .message
                .contains("degraded Claude Code delivery append failed")
        }));
    }

    #[test]
    fn send_non_claude_sqlite_failure_delivers_original_and_error_via_outbound_boundary() {
        let runtime = TestRuntime::new(
            Some("sqlite write failed"),
            None,
            DeliveryHarnessPath::NonClaude,
        );
        let observability = RecordingObservability::default();
        let tempdir = tempdir().expect("tempdir");

        let outcome = super::send_mail_with_runtime_impl(
            send_request(tempdir.path()),
            &observability,
            &runtime,
        )
        .expect("send outcome");

        assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
        assert_eq!(outcome.warnings.len(), 1);
        assert!(
            runtime
                .appended_messages
                .lock()
                .expect("append lock")
                .is_empty()
        );
        let deliveries = runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock");
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].team.as_str(), TEST_TEAM);
        assert_eq!(deliveries[0].agent.as_str(), "recipient");
        assert_eq!(deliveries[0].messages.len(), 2);
        assert_eq!(deliveries[0].messages[0].from.as_str(), TEST_SENDER);
        assert_eq!(deliveries[0].messages[1].from.as_str(), "atm-system");
        drop(deliveries);
        let captures = runtime.hook_captures.lock().expect("hook capture lock");
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[0].sender.as_str(), TEST_SENDER);
        assert!(captures[0].sender_team.is_some());
        assert_eq!(captures[0].recipient.agent.as_str(), "recipient");
        assert_eq!(captures[0].recipient.team.as_str(), TEST_TEAM);
        assert!(captures[0].message_id.to_string().len() > 10);
        let _requires_ack = captures[0].requires_ack;
        let _is_ack = captures[0].is_ack;
        assert_eq!(
            captures[0]
                .task_id
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("task-123")
        );
        assert_eq!(captures[1].sender.as_str(), "atm-system");
        drop(captures);

        let events = observability.events.lock().expect("events lock");
        assert!(events.iter().any(|event| {
            event.command == "delivery_policy"
                && event.outcome == "delivery_policy.new_message.non_claude_original"
        }));
        assert!(events.iter().any(|event| {
            event.command == "delivery_policy"
                && event.outcome == "delivery_policy.new_message.non_claude_error"
        }));
    }

    #[test]
    fn send_non_claude_success_delivers_original_via_outbound_boundary() {
        let runtime = TestRuntime::new(None, None, DeliveryHarnessPath::NonClaude);
        let observability = RecordingObservability::default();
        let tempdir = tempdir().expect("tempdir");

        let outcome = super::send_mail_with_runtime_impl(
            send_request(tempdir.path()),
            &observability,
            &runtime,
        )
        .expect("send outcome");

        assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
        assert!(outcome.warnings.is_empty());
        assert!(
            runtime
                .appended_messages
                .lock()
                .expect("append lock")
                .is_empty()
        );
        let deliveries = runtime
            .non_claude_deliveries
            .lock()
            .expect("non-claude deliveries lock");
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].messages.len(), 1);
        assert_eq!(deliveries[0].messages[0].from.as_str(), TEST_SENDER);
    }

    #[test]
    fn send_append_failure_routes_to_post_send_hook_fallback() {
        let runtime =
            TestRuntime::new(None, Some("append failed"), DeliveryHarnessPath::ClaudeCode);
        let observability = RecordingObservability::default();
        let tempdir = tempdir().expect("tempdir");

        let outcome = super::send_mail_with_runtime_impl(
            send_request(tempdir.path()),
            &observability,
            &runtime,
        )
        .expect("send outcome");

        assert_eq!(outcome.outcome, SendCommandOutcome::Sent);
        assert_eq!(outcome.warnings.len(), 1);
        let captures = runtime.hook_captures.lock().expect("hook capture lock");
        assert_eq!(captures.len(), 1);
        assert_eq!(captures[0].sender.as_str(), TEST_SENDER);
        drop(captures);

        let events = observability.events.lock().expect("events lock");
        assert!(events.iter().any(|event| {
            event.command == "delivery_policy"
                && event.outcome == "delivery_policy.new_message.post_send_hook_fallback"
        }));
    }

    #[test]
    fn save_send_alert_state_round_trips() {
        let tempdir = tempdir().expect("tempdir");
        let path = alert_state::state_path(tempdir.path());
        let mut state = alert_state::SendAlertState::default();
        state
            .missing_team_config_keys
            .insert(format!("teams/{TEST_TEAM}/config.json"));

        alert_state::save(&path, &state).expect("save");
        let loaded = alert_state::load(&path).expect("load");
        assert_eq!(
            loaded.missing_team_config_keys,
            state.missing_team_config_keys
        );
    }

    #[test]
    fn process_is_alive_reports_current_process() {
        assert!(process_is_alive(std::process::id()));
    }

    #[test]
    fn acquire_send_alert_lock_evicts_stale_pid_lock() {
        let tempdir = tempdir().expect("tempdir");
        let path = alert_state::lock_path(tempdir.path());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("lock dir");
        }
        fs::write(&path, u32::MAX.to_string()).expect("stale lock");

        let guard = alert_state::acquire_lock(&path).expect("acquire lock");
        let pid = fs::read_to_string(&path).expect("lock contents");
        assert_eq!(pid.trim(), std::process::id().to_string());
        drop(guard);
        assert!(!path.exists());
    }

    #[test]
    fn send_request_new_rejects_invalid_recipient_before_command_execution() {
        let tempdir = tempdir().expect("tempdir");
        let error = SendRequest::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            Some(ROLE_TEAM_LEAD),
            "../evil",
            Some(TEST_TEAM),
            SendMessageSource::Inline("hello".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect_err("invalid address");

        assert!(error.message.contains("agent name"));
    }

    #[test]
    fn send_request_new_rejects_invalid_team_override_before_command_execution() {
        let tempdir = tempdir().expect("tempdir");
        let error = SendRequest::new(
            tempdir.path().to_path_buf(),
            tempdir.path().to_path_buf(),
            Some(ROLE_TEAM_LEAD),
            TEST_SENDER,
            Some("../evil"),
            SendMessageSource::Inline("hello".to_string()),
            None,
            false,
            None,
            false,
        )
        .expect_err("invalid team");

        assert!(error.message.contains("team name"));
    }

    #[test]
    fn prepare_threaded_message_reopens_ack_for_ack_required_thread() {
        let root_id = AtmMessageId::new();
        let mut root = message(TEST_SENDER, root_id, None, None);
        root.acknowledged_at = Some(IsoTimestamp::now());
        let mut update = message(
            TEST_SENDER,
            AtmMessageId::new(),
            Some(root_id),
            Some(ThreadMode::AddDetails),
        );

        prepare_threaded_message(&mut update, &[root]).expect("prepare update");

        assert!(update.pending_ack_at.is_some());
        assert!(update.acknowledged_at.is_none());
    }

    #[test]
    fn prepare_threaded_message_reopens_ack_for_ack_required_supersede_thread() {
        let root_id = AtmMessageId::new();
        let mut root = message(TEST_SENDER, root_id, None, None);
        root.acknowledged_at = Some(IsoTimestamp::now());
        let mut update = message(
            TEST_SENDER,
            AtmMessageId::new(),
            Some(root_id),
            Some(ThreadMode::Supersede),
        );

        prepare_threaded_message(&mut update, &[root]).expect("prepare update");

        assert!(update.pending_ack_at.is_some());
        assert!(update.acknowledged_at.is_none());
    }

    #[test]
    fn prepare_threaded_message_rejects_non_originating_sender() {
        let root_id = AtmMessageId::new();
        let root = message(TEST_SENDER, root_id, None, None);
        let mut update = message(
            ROLE_TEAM_LEAD,
            AtmMessageId::new(),
            Some(root_id),
            Some(ThreadMode::Supersede),
        );

        let error = prepare_threaded_message(&mut update, &[root]).expect_err("different sender");

        assert!(error.message.contains("original sender"));
    }
}
