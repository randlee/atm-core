//! Send command service implementation and post-send hook handling.

use std::iter;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Map;
use tracing::warn;

use crate::address::AgentAddress;
use crate::boundary;
use crate::config;
use crate::error::{AtmError, AtmErrorCode};
use crate::identity;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::{AtmMessageId, MessageEnvelope, ThreadMode};
use crate::service_runtime::{LocalServiceRuntime, RetainedServiceRuntime};
use crate::service_runtime_store::{RetainedMailboxRuntime, default_runtime};
use crate::threading::{ThreadIndex, canonical_sender_identity, is_ephemeral};
use crate::types::{AgentName, CommandAction, IsoTimestamp, TaskId, TeamName};
use crate::workflow;

mod alert_state;
pub(crate) mod file_policy;
pub(super) mod hook;
pub(crate) mod input;
pub(crate) mod summary;

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
    pub outcome: String,
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

/// Intermediate state produced by team-config validation, consumed by the send body.
struct ValidatedSendContext {
    inbox_path: std::path::PathBuf,
    warnings: Vec<WarningEntry>,
}

/// Validate team config presence and membership for the resolved recipient.
///
/// Returns the inbox path and any accumulated warnings.  On success the
/// caller owns `warnings` so they can be forwarded to the final outcome.
fn validate_team_config<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    recipient: &ResolvedRecipient,
    team_dir: &std::path::Path,
) -> Result<ValidatedSendContext, AtmError> {
    let inbox_path = runtime.inbox_path(&request.home_dir, &recipient.team, &recipient.agent)?;
    let mut warnings = Vec::new();

    match runtime.load_team_config(team_dir) {
        Ok(team_config) => {
            alert_state::clear_missing_team_config_alert(
                &request.home_dir,
                &alert_state::missing_team_config_alert_key(team_dir),
            );
            if !team_config
                .members
                .iter()
                .any(|member| member.name == recipient.agent.as_str())
            {
                return Err(AtmError::agent_not_found(&recipient.agent, &recipient.team));
            }
        }
        Err(error) if error.is_missing_document() => {
            if !inbox_path.exists() {
                return Err(AtmError::missing_document(format!(
                    "team config is missing at {} and inbox {} does not exist, so send cannot safely proceed",
                    team_dir.join("config.json").display(),
                    inbox_path.display()
                ))
                .with_recovery(
                    "Restore config.json for the team or create the intended inbox by an approved workflow before retrying.",
                ));
            }

            warnings.push(WarningEntry::new(
                format!(
                    "warning: team config is missing at {}; send used existing inbox fallback for {}@{}.",
                    team_dir.join("config.json").display(),
                    recipient.agent,
                    recipient.team
                ),
                Some("Restore the team config."),
            ));
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
        }
        Err(error) => return Err(error),
    }

    Ok(ValidatedSendContext {
        inbox_path,
        warnings,
    })
}

/// Build the [`MessageEnvelope`] and persist it via [`persist_message_and_seed_workflow`].
///
/// This is the sole write-boundary owner for the send command path.
#[allow(clippy::too_many_arguments)]
fn build_and_persist_envelope<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    runtime: &R,
    request: &SendRequest,
    recipient: &ResolvedRecipient,
    inbox_path: &std::path::Path,
    display_sender: &AgentName,
    sender_team: Option<&TeamName>,
    body: &str,
    summary: &str,
    message_id: AtmMessageId,
    timestamp: IsoTimestamp,
    requires_ack: bool,
) -> Result<(), AtmError> {
    let envelope = MessageEnvelope {
        from: display_sender.clone(),
        text: body.to_owned(),
        timestamp,
        read: false,
        source_team: sender_team
            .cloned()
            .or_else(|| Some(recipient.team.clone())),
        summary: Some(summary.to_owned()),
        message_id: Some(message_id),
        pending_ack_at: requires_ack.then_some(timestamp),
        acknowledged_at: None,
        acknowledges_message_id: None,
        parent_message_id: request.parent_message_id,
        thread_mode: request.thread_mode,
        expires_at: request.expires_at,
        task_id: request.task_id.clone(),
        extra: Map::new(),
    };
    persist_message_and_seed_workflow(
        runtime,
        &request.home_dir,
        &recipient.team,
        &recipient.agent,
        inbox_path,
        &envelope,
        false,
    )
}

fn send_mail_with_runtime_impl<R: RetainedServiceRuntime + RetainedMailboxRuntime>(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
    runtime: &R,
) -> Result<SendOutcome, AtmError> {
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

    let ValidatedSendContext {
        inbox_path,
        warnings,
    } = validate_team_config(runtime, &request, &recipient, &team_dir)?;

    let task_id = request.task_id.clone();
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override.clone());
    let message_id = AtmMessageId::new();
    let timestamp = IsoTimestamp::now();

    if !request.dry_run {
        build_and_persist_envelope(
            runtime,
            &request,
            &recipient,
            &inbox_path,
            &display_sender,
            sender_team.as_ref(),
            &body,
            &summary,
            message_id,
            timestamp,
            requires_ack,
        )?;
    }

    let command_outcome = if request.dry_run { "dry_run" } else { "sent" };
    let mut outcome = SendOutcome {
        action: CommandAction::Send,
        team: recipient.team.clone(),
        agent: recipient.agent.clone(),
        sender: canonical_sender.clone(),
        outcome: command_outcome.to_string(),
        message_id,
        requires_ack,
        task_id: task_id.clone(),
        summary: Some(summary),
        message: request.dry_run.then_some(body),
        warnings,
        dry_run: request.dry_run,
    };

    if !request.dry_run {
        runtime.maybe_run_post_send_hook(
            &mut outcome.warnings,
            config.as_ref(),
            PostSendHookContext {
                sender: &canonical_sender,
                sender_team: sender_team.as_ref(),
                recipient: &recipient,
                recipient_pane_id: None,
                message_id,
                requires_ack,
                is_ack: false,
                task_id: task_id.as_ref(),
            },
        );
    }

    if let Err(error) = observability.emit(CommandEvent {
        command: "send",
        action: "send",
        outcome: command_outcome,
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: canonical_sender,
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
        error_code: None,
        error_message: None,
    }) {
        warn!(%error, command = "send", action = "send", "failed to emit send command event");
    }

    Ok(outcome)
}

#[derive(Debug)]
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
    let team_lead_inbox = match runtime.inbox_path(home_dir, team, &team_lead_agent) {
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

    let notice = MessageEnvelope {
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
    };

    if let Err(error) = persist_message_and_seed_workflow(
        runtime,
        home_dir,
        team,
        &AgentName::from_validated(ROLE_TEAM_LEAD),
        &team_lead_inbox,
        &notice,
        true,
    ) {
        warn!(
            code = %AtmErrorCode::WarningMissingTeamConfigFallback,
            %error,
            path = %team_lead_inbox.display(),
            team = %team,
            "failed to persist missing-config notice via shared mailbox/workflow commit path"
        );
    }
}

pub(crate) fn persist_message_and_seed_workflow(
    runtime: &(impl RetainedServiceRuntime + RetainedMailboxRuntime),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    inbox_path: &Path,
    envelope: &MessageEnvelope,
    require_existing_inbox: bool,
) -> Result<(), AtmError> {
    if require_existing_inbox && !inbox_path.exists() {
        return Ok(());
    }

    let mut prepared = envelope.clone();
    let inbox_messages = load_store_backed_mailbox_projection(runtime, home_dir, team, agent)?;
    prepare_threaded_message(&mut prepared, &inbox_messages)?;

    runtime.commit_workflow_state(
        home_dir,
        team,
        agent,
        iter::empty(),
        runtime.mailbox_timeout_policy().workflow_lock_timeout,
        |workflow_state| {
            mirror_message_to_store(runtime, team, agent, &prepared)?;
            Ok((
                (),
                workflow::remember_initial_state(workflow_state, &prepared),
            ))
        },
    )?;
    runtime.refresh_compat_inbox_projection(home_dir, team, agent)
}

fn load_store_backed_mailbox_projection(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<MessageEnvelope>, AtmError> {
    let mut metadata_rows = runtime.query_mailbox_metadata_rows(home_dir, team, agent, None)?;
    metadata_rows.sort_by(|left, right| {
        left.message_at
            .cmp(&right.message_at)
            .then_with(|| left.message_key.as_ref().cmp(right.message_key.as_ref()))
    });

    metadata_rows
        .into_iter()
        .map(|row| {
            runtime
                .load_message_record(home_dir, team, agent, &row.message_key)?
                .map(|record| record.envelope)
                .ok_or_else(|| {
                    AtmError::validation(format!(
                        "sqlite mailbox metadata row {} could not be reloaded for compatibility inbox export",
                        row.message_key
                    ))
                    .with_recovery(
                        "Repair or remove the malformed sqlite mailbox row before retrying the ATM command.",
                    )
                })
        })
        .collect()
}

fn mirror_message_to_store(
    runtime: &(impl RetainedMailboxRuntime + ?Sized),
    team: &TeamName,
    agent: &AgentName,
    envelope: &MessageEnvelope,
) -> Result<(), AtmError> {
    let Some(message_id) = envelope.message_id else {
        return Ok(());
    };
    let message_key = boundary::MessageKey::new(format!("atm:{message_id}"))?;
    runtime.persist_message_record(boundary::MailStoreMessageRecord {
        team: team.clone(),
        agent: agent.clone(),
        message_key: message_key.clone(),
        envelope: envelope.clone(),
    })?;
    runtime.persist_message_state(boundary::MailMessageState {
        team: team.clone(),
        agent: agent.clone(),
        actor: agent.clone(),
        message_key,
        read: envelope.read,
        pending_ack_at: envelope.pending_ack_at,
        acknowledged_at: envelope.acknowledged_at,
        expires_at: envelope.expires_at,
        deleted_at: None,
        updated_at: Some(IsoTimestamp::now()),
    })
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
    use serde_json::Map;
    use std::cell::Cell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use tempfile::tempdir;

    use super::{alert_state, prepare_threaded_message, send_mail_with_runtime_impl};
    use crate::boundary;
    use crate::config::AtmConfig;
    use crate::error::AtmError;
    use crate::observability::NullObservability;
    use crate::process::process_is_alive;
    use crate::roles::ROLE_TEAM_LEAD;
    use crate::schema::{AtmMessageId, MessageEnvelope, TeamConfig, ThreadMode};
    use crate::send::{SendMessageSource, SendRequest, WarningEntry};
    use crate::service_runtime::{RetainedMailboxTimeoutPolicy, RetainedServiceRuntime};
    use crate::service_runtime_store::RetainedMailboxRuntime;
    use crate::test_support::{TEST_SENDER, TEST_TEAM};
    use crate::types::{AgentName, IsoTimestamp, TeamName};
    use crate::workflow::WorkflowStateFile;

    /// Minimal test double for `RetainedServiceRuntime + RetainedMailboxRuntime`.
    ///
    /// Uses a real temp directory so path-existence checks pass.  Tracks
    /// whether any write methods are invoked so tests can assert on the
    /// write-boundary invariant.
    struct StubRuntime {
        home_dir: PathBuf,
        write_called: Cell<bool>,
        compat_export_called: Cell<bool>,
    }

    impl StubRuntime {
        fn new(home_dir: PathBuf, team: &str, agent: &str) -> Self {
            // Create the minimal team dir and inbox so path-existence checks pass.
            let team_dir = home_dir.join("teams").join(team);
            fs::create_dir_all(&team_dir).expect("team dir");
            let inbox_dir = team_dir.join(agent);
            fs::create_dir_all(&inbox_dir).expect("inbox dir");
            // Write a minimal config.json with one member entry.
            let config_json = format!(
                r#"{{"members":[{{"name":"{agent}","agent_id":"","agent_type":"","model":"","tmux_pane_id":"","cwd":""}}]}}"#
            );
            fs::write(team_dir.join("config.json"), config_json).expect("team config");
            Self {
                home_dir,
                write_called: Cell::new(false),
                compat_export_called: Cell::new(false),
            }
        }
    }

    impl RetainedServiceRuntime for StubRuntime {
        fn load_config(&self, _current_dir: &Path) -> Result<Option<AtmConfig>, AtmError> {
            Ok(None)
        }

        fn load_team_config(&self, team_dir: &Path) -> Result<TeamConfig, AtmError> {
            let path = team_dir.join("config.json");
            let raw = fs::read_to_string(&path).map_err(|_| {
                AtmError::missing_document(format!("team config not found at {}", path.display()))
            })?;
            serde_json::from_str(&raw).map_err(|e| AtmError::config(e.to_string()))
        }

        fn team_dir(&self, _home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
            Ok(self.home_dir.join("teams").join(team.as_str()))
        }

        fn inbox_path(
            &self,
            _home_dir: &Path,
            team: &TeamName,
            agent: &AgentName,
        ) -> Result<PathBuf, AtmError> {
            Ok(self
                .home_dir
                .join("teams")
                .join(team.as_str())
                .join(agent.as_str()))
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
                workflow_lock_timeout: Duration::from_secs(5),
            }
        }

        fn maybe_run_post_send_hook(
            &self,
            _warnings: &mut Vec<WarningEntry>,
            _config: Option<&AtmConfig>,
            _context: crate::send::PostSendHookContext<'_>,
        ) {
            // No-op in tests.
        }

        fn refresh_compat_inbox_projection(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
        ) -> Result<(), AtmError> {
            self.compat_export_called.set(true);
            Ok(())
        }

        fn commit_workflow_state<T, I, F>(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _extra_write_paths: I,
            _timeout: Duration,
            _body: F,
        ) -> Result<T, AtmError>
        where
            I: IntoIterator<Item = PathBuf>,
            F: FnOnce(&mut WorkflowStateFile) -> Result<(T, bool), AtmError>,
        {
            Err(AtmError::validation(
                "StubRuntime: commit_workflow_state not exercised in dry-run tests",
            ))
        }
    }

    impl RetainedMailboxRuntime for StubRuntime {
        fn query_mailbox_metadata_rows(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _limit: Option<usize>,
        ) -> Result<Vec<boundary::MailStoreMailboxMetadataRow>, AtmError> {
            Ok(vec![])
        }

        fn load_message_record(
            &self,
            _home_dir: &Path,
            _team: &TeamName,
            _agent: &AgentName,
            _message_key: &boundary::MessageKey,
        ) -> Result<Option<boundary::MailStoreMessageRecord>, AtmError> {
            Ok(None)
        }

        fn persist_message_record(
            &self,
            _record: boundary::MailStoreMessageRecord,
        ) -> Result<(), AtmError> {
            self.write_called.set(true);
            Ok(())
        }

        fn persist_message_state(
            &self,
            _state: boundary::MailMessageState,
        ) -> Result<(), AtmError> {
            self.write_called.set(true);
            Ok(())
        }
    }

    fn stub_send_request(home_dir: PathBuf, dry_run: bool) -> SendRequest {
        SendRequest {
            home_dir,
            current_dir: PathBuf::from("."),
            sender_override: Some(AgentName::from_validated(TEST_SENDER)),
            to: format!("{TEST_SENDER}@{TEST_TEAM}")
                .parse()
                .expect("address"),
            team_override: None,
            message_source: SendMessageSource::Inline("hello world".to_string()),
            summary_override: None,
            requires_ack: false,
            task_id: None,
            parent_message_id: None,
            thread_mode: None,
            expires_at: None,
            dry_run,
        }
    }

    #[test]
    fn send_succeeds_without_command_owned_mailbox_rewrite() {
        // Verify the send command path with dry_run=true completes successfully
        // without triggering any write operations on mailbox records.  The command
        // path must not rewrite the source (sender) inbox — all writes are owned
        // by the `build_and_persist_envelope` helper which is gated on !dry_run.
        let tempdir = tempdir().expect("tempdir");
        let stub = StubRuntime::new(tempdir.path().to_path_buf(), TEST_TEAM, TEST_SENDER);
        let request = stub_send_request(tempdir.path().to_path_buf(), true);

        let outcome = send_mail_with_runtime_impl(request, &NullObservability, &stub)
            .expect("dry-run send should succeed");

        assert!(outcome.dry_run, "outcome must reflect dry-run flag");
        assert!(
            !stub.write_called.get(),
            "send command path must not invoke mailbox write operations"
        );
    }

    #[test]
    fn only_approved_owner_path_produces_runtime_compatibility_export() {
        // Verify that `refresh_compat_inbox_projection` is never called directly
        // from the send command path.  It is only reachable through
        // `persist_message_and_seed_workflow`, which is gated on !dry_run.
        // With dry_run=true the compat export must not be triggered.
        let tempdir = tempdir().expect("tempdir");
        let stub = StubRuntime::new(tempdir.path().to_path_buf(), TEST_TEAM, TEST_SENDER);
        let request = stub_send_request(tempdir.path().to_path_buf(), true);

        send_mail_with_runtime_impl(request, &NullObservability, &stub)
            .expect("dry-run send should succeed");

        assert!(
            !stub.compat_export_called.get(),
            "compatibility export must not be triggered from the command-owned send path"
        );
    }

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
