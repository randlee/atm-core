//! Send command service implementation and post-send hook handling.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Map;
use tracing::warn;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorCode};
use crate::home;
use crate::identity;
use crate::inbox_export;
use crate::inbox_export::ExportEventContext;
use crate::mail_store::{AckStateRecord, MailStore, MessageSourceKind, StoredMessageRecord};
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::roster_store::{RosterMemberRecord, RosterStore};
use crate::schema::{AtmMessageId, LegacyMessageId, MessageEnvelope};
use crate::store::{
    InsertOutcome, MessageKey, RecipientPaneId, StoreDuplicateIdentity, StoreError,
};
use crate::task_store::{TaskRecord, TaskStatus, TaskStore};
use crate::team_ingress;
use crate::types::{AgentName, TaskId, TeamName};
use crate::workflow;

mod alert_state;
pub(crate) mod file_policy;
pub(super) mod hook;
pub(crate) mod input;
pub(crate) mod summary;

#[derive(Debug, Clone)]
pub enum SendMessageSource {
    Inline(String),
    Stdin,
    File {
        path: PathBuf,
        message: Option<String>,
    },
}

#[derive(Debug, Clone)]
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
            dry_run,
        })
    }
}

/// Result of sending one ATM mailbox message.
#[derive(Debug, Clone, Serialize)]
pub struct SendOutcome {
    pub action: &'static str,
    pub team: TeamName,
    pub agent: AgentName,
    pub sender: AgentName,
    pub outcome: &'static str,
    pub message_id: LegacyMessageId,
    pub requires_ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    // TODO(v1.1.0): Replace this Vec<String> with a structured WarningEntry type
    // so degraded-mode warnings can carry recovery guidance separately from the
    // rendered message text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
}

pub fn resolve_store_team(request: &SendRequest) -> Result<TeamName, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    request
        .to
        .team
        .as_deref()
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            config::resolve_team(
                request.team_override.as_ref().map(|team| team.as_str()),
                config.as_ref(),
            )
        })
        .ok_or_else(AtmError::team_unavailable)
}

#[derive(Debug)]
struct PreparedSend {
    home_dir: PathBuf,
    config: Option<config::AtmConfig>,
    canonical_sender: AgentName,
    recipient: ResolvedRecipient,
    sender_team: Option<TeamName>,
    display_sender: String,
    team_dir: PathBuf,
    inbox_path: PathBuf,
    warnings: Vec<String>,
    task_id: Option<TaskId>,
    requires_ack: bool,
    body: String,
    summary: String,
    message_id: LegacyMessageId,
    atm_message_id: AtmMessageId,
    timestamp: crate::types::IsoTimestamp,
    dry_run: bool,
}

impl PreparedSend {
    fn team_config_present(&self) -> bool {
        self.team_dir.join("config.json").exists()
    }

    fn home_dir(&self) -> PathBuf {
        self.home_dir.clone()
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
    let prepared = prepare_send_request(request)?;
    let envelope = build_outgoing_envelope(&prepared);

    if !prepared.dry_run {
        append_mailbox_message_and_seed_workflow(
            &prepared.home_dir(),
            &prepared.recipient.team,
            &prepared.recipient.agent,
            &prepared.inbox_path,
            &envelope,
        )?;
    }

    let mut outcome = build_send_outcome(&prepared);
    finalize_send(&prepared, &mut outcome, observability, None);
    Ok(outcome)
}

pub fn send_mail_via_store<S>(
    request: SendRequest,
    store: &S,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError>
where
    S: MailStore + TaskStore + RosterStore,
{
    let prepared = prepare_send_request(request)?;
    let envelope = build_outgoing_envelope(&prepared);

    let recipient_pane_id = if prepared.team_config_present() {
        let roster = team_ingress::ingest_team_config(
            &prepared.team_dir,
            &prepared.recipient.team,
            store,
            &team_ingress::default_host_name(),
        )?;
        roster_member_for_recipient(&roster, &prepared.recipient.agent)
            .and_then(|member| member.recipient_pane_id.clone())
    } else {
        None
    };

    if !prepared.dry_run {
        let stored = stored_message_record(&prepared, &envelope)?;
        match store
            .insert_message(&stored)
            .map_err(|error| map_store_error("failed to insert outbound message row", error))?
        {
            InsertOutcome::Inserted(_) => {}
            InsertOutcome::Duplicate(identity) => {
                return Err(duplicate_send_error(identity));
            }
        }

        if prepared.requires_ack {
            store
                .upsert_ack_state(&AckStateRecord {
                    message_key: MessageKey::from_atm_message_id(prepared.atm_message_id),
                    pending_ack_at: Some(prepared.timestamp),
                    acknowledged_at: None,
                    ack_reply_message_key: None,
                    ack_reply_team: None,
                    ack_reply_agent: None,
                })
                .map_err(|error| map_store_error("failed to persist outbound ack state", error))?;
        }

        if let Some(task_id) = prepared.task_id.clone() {
            store
                .upsert_task(&TaskRecord {
                    task_id,
                    message_key: MessageKey::from_atm_message_id(prepared.atm_message_id),
                    status: TaskStatus::PendingAck,
                    created_at: prepared.timestamp,
                    acknowledged_at: None,
                    metadata_json: None,
                })
                .map_err(|error| map_store_error("failed to persist outbound task row", error))?;
        }

        inbox_export::export_message(
            &prepared.home_dir(),
            &prepared.recipient.team,
            &prepared.recipient.agent,
            &envelope,
            observability,
            ExportEventContext {
                command: "send",
                sender: prepared.canonical_sender.to_string(),
                message_id: Some(prepared.message_id),
                requires_ack: prepared.requires_ack,
                task_id: prepared.task_id.clone(),
            },
        )?;
    }

    let mut outcome = build_send_outcome(&prepared);
    finalize_send(
        &prepared,
        &mut outcome,
        observability,
        recipient_pane_id.as_ref(),
    );
    Ok(outcome)
}

fn prepare_send_request(request: SendRequest) -> Result<PreparedSend, AtmError> {
    let config = config::load_config(&request.current_dir)?;
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
        request.sender_override.as_deref(),
        sender_team.as_deref(),
        &recipient.team,
        config.as_ref(),
    );

    let team_dir = home::team_dir_from_home(&request.home_dir, &recipient.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&recipient.team));
    }

    let inbox_path =
        home::inbox_path_from_home(&request.home_dir, &recipient.team, &recipient.agent)?;
    let mut warnings = Vec::new();

    match config::load_team_config(&team_dir) {
        Ok(team_config) => {
            alert_state::clear_missing_team_config_alert(
                &request.home_dir,
                &alert_state::missing_team_config_alert_key(&team_dir),
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

            warnings.push(format!(
                "warning: team config is missing at {}; send used existing inbox fallback for {}@{}. Restore the team config.",
                team_dir.join("config.json").display(),
                recipient.agent,
                recipient.team
            ));
            warn!(code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                config_path = %team_dir.join("config.json").display(),
                recipient = %recipient.agent,
                team = %recipient.team,
                "send used existing inbox fallback; team config is missing"
            );

            if !request.dry_run {
                notify_team_lead_missing_config(
                    &request.home_dir,
                    &team_dir,
                    &recipient.team,
                    &recipient.agent,
                );
            }
        }
        Err(error) => return Err(error),
    }

    let task_id = request.task_id;
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override);
    let message_id = LegacyMessageId::new();
    let (atm_message_id, timestamp) = AtmMessageId::new_with_timestamp();

    Ok(PreparedSend {
        home_dir: request.home_dir,
        config,
        canonical_sender,
        recipient,
        sender_team,
        display_sender,
        team_dir,
        inbox_path,
        warnings,
        task_id,
        requires_ack,
        body,
        summary,
        message_id,
        atm_message_id,
        timestamp,
        dry_run: request.dry_run,
    })
}

fn build_outgoing_envelope(prepared: &PreparedSend) -> MessageEnvelope {
    let mut extra = Map::new();
    workflow::set_atm_message_id(&mut extra, prepared.atm_message_id);
    if prepared.display_sender != prepared.canonical_sender.as_str() {
        set_canonical_sender_metadata(&mut extra, &prepared.canonical_sender);
    }
    MessageEnvelope {
        from: prepared
            .display_sender
            .parse()
            .expect("display sender is valid"),
        text: prepared.body.clone(),
        timestamp: prepared.timestamp,
        read: false,
        source_team: prepared
            .sender_team
            .clone()
            .or_else(|| Some(prepared.recipient.team.clone())),
        summary: Some(prepared.summary.clone()),
        message_id: Some(prepared.message_id),
        pending_ack_at: prepared.requires_ack.then_some(prepared.timestamp),
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: prepared.task_id.clone(),
        extra,
    }
}

fn build_send_outcome(prepared: &PreparedSend) -> SendOutcome {
    SendOutcome {
        action: "send",
        team: prepared.recipient.team.clone(),
        agent: prepared.recipient.agent.clone(),
        sender: prepared.canonical_sender.clone(),
        outcome: if prepared.dry_run { "dry_run" } else { "sent" },
        message_id: prepared.message_id,
        requires_ack: prepared.requires_ack,
        task_id: prepared.task_id.clone(),
        summary: Some(prepared.summary.clone()),
        message: prepared.dry_run.then_some(prepared.body.clone()),
        warnings: prepared.warnings.clone(),
        dry_run: prepared.dry_run,
    }
}

fn finalize_send(
    prepared: &PreparedSend,
    outcome: &mut SendOutcome,
    observability: &dyn ObservabilityPort,
    recipient_pane_id: Option<&RecipientPaneId>,
) {
    if !prepared.dry_run {
        maybe_run_post_send_hook(
            &mut outcome.warnings,
            prepared.config.as_ref(),
            PostSendHookContext {
                sender: &prepared.canonical_sender,
                sender_team: prepared.sender_team.as_ref(),
                recipient: &prepared.recipient,
                message_id: prepared.message_id,
                requires_ack: prepared.requires_ack,
                is_ack: false,
                task_id: prepared.task_id.as_ref(),
                recipient_pane_id,
            },
        );
    }

    let _ = observability.emit(CommandEvent {
        command: "send",
        action: "send",
        outcome: outcome.outcome,
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: prepared.canonical_sender.to_string(),
        message_id: Some(outcome.message_id),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id: prepared.task_id.clone(),
        error_code: None,
        error_message: None,
    });
}

fn roster_member_for_recipient<'a>(
    roster: &'a [RosterMemberRecord],
    recipient: &AgentName,
) -> Option<&'a RosterMemberRecord> {
    roster.iter().find(|member| &member.agent_name == recipient)
}

fn stored_message_record(
    prepared: &PreparedSend,
    envelope: &MessageEnvelope,
) -> Result<StoredMessageRecord, AtmError> {
    let raw_metadata_json = envelope
        .extra
        .get("metadata")
        .map(serde_json::to_string)
        .transpose()
        .map_err(|source| {
            AtmError::new(
                crate::error::AtmErrorKind::Serialization,
                format!(
                    "failed to encode ATM metadata for outbound message to {}",
                    prepared.recipient.agent
                ),
            )
            .with_source(source)
        })?;

    Ok(StoredMessageRecord {
        message_key: MessageKey::from_atm_message_id(prepared.atm_message_id),
        team_name: prepared.recipient.team.clone(),
        recipient_agent: prepared.recipient.agent.clone(),
        sender_display: prepared.display_sender.clone(),
        sender_canonical: Some(prepared.canonical_sender.clone()),
        sender_team: prepared
            .sender_team
            .clone()
            .or_else(|| Some(prepared.recipient.team.clone())),
        body: prepared.body.clone(),
        summary: Some(prepared.summary.clone()),
        created_at: prepared.timestamp,
        source_kind: MessageSourceKind::Atm,
        legacy_message_id: Some(prepared.message_id),
        atm_message_id: Some(prepared.atm_message_id),
        raw_metadata_json,
    })
}

fn duplicate_send_error(identity: StoreDuplicateIdentity) -> AtmError {
    AtmError::new_with_code(
        AtmErrorCode::StoreConstraintViolation,
        crate::error::AtmErrorKind::MailboxWrite,
        format!("generated duplicate outbound message identity during send: {identity:?}"),
    )
    .with_recovery(
        "Retry the send once. If the duplicate persists, inspect the SQLite store for a stale message identity collision before sending again.",
    )
}

fn map_store_error(context: &str, error: StoreError) -> AtmError {
    let mut atm_error = AtmError::new_with_code(
        error.code,
        crate::error::AtmErrorKind::MailboxWrite,
        format!("{context}: {}", error.message),
    );
    if let Some(recovery) = error.recovery.as_ref() {
        atm_error = atm_error.with_recovery(recovery.clone());
    }
    atm_error.with_source(error)
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
    pub(crate) message_id: LegacyMessageId,
    pub(crate) requires_ack: bool,
    pub(crate) is_ack: bool,
    pub(crate) task_id: Option<&'a TaskId>,
    pub(crate) recipient_pane_id: Option<&'a RecipientPaneId>,
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
        SendMessageSource::File { path, message } => file_policy::process_file_reference(
            path,
            message.as_deref(),
            team_name,
            current_dir,
            home_dir,
        ),
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn notify_team_lead_missing_config(
    home_dir: &Path,
    team_dir: &Path,
    team: &TeamName,
    recipient: &AgentName,
) {
    let alert_key = alert_state::missing_team_config_alert_key(team_dir);
    if !alert_state::register_missing_team_config_alert(home_dir, &alert_key) {
        return;
    }

    let team_lead_inbox = match home::inbox_path_from_home(home_dir, team, "team-lead") {
        Ok(path) => path,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::WarningMissingTeamConfigFallback,
                %error,
                team = %team,
                "failed to resolve team-lead inbox for missing-config notice"
            );
            return;
        }
    };

    if !team_lead_inbox.exists() {
        return;
    }

    let config_path = team_dir.join("config.json");
    let (atm_message_id, timestamp) = AtmMessageId::new_with_timestamp();
    let mut extra = Map::new();
    workflow::set_atm_message_id(&mut extra, atm_message_id);
    extra.insert(
        "atmAlertKind".into(),
        serde_json::Value::String("missing_team_config".into()),
    );
    extra.insert(
        "missingConfigPath".into(),
        serde_json::Value::String(config_path.display().to_string()),
    );

    let notice = MessageEnvelope {
        from: "atm-identity-missing"
            .parse()
            .expect("system sender is valid"),
        text: format!(
            "ATM warning: send used existing inbox fallback for {recipient}@{team} because team config is missing at {}. Please restore config.json.",
            config_path.display()
        ),
        timestamp,
        read: false,
        source_team: Some(team.parse().expect("team name")),
        summary: Some(format!(
            "ATM warning: missing team config fallback used for {recipient}@{team}"
        )),
        message_id: Some(LegacyMessageId::new()),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: None,
        task_id: None,
        extra,
    };

    if let Err(error) = append_mailbox_message_and_seed_workflow(
        home_dir,
        team,
        &AgentName::from_validated("team-lead"),
        &team_lead_inbox,
        &notice,
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

fn append_mailbox_message_and_seed_workflow(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
    inbox_path: &Path,
    envelope: &MessageEnvelope,
) -> Result<(), AtmError> {
    workflow::commit_workflow_state(
        home_dir,
        team,
        agent,
        [inbox_path.to_path_buf()],
        mailbox::lock::default_lock_timeout(),
        |workflow_state| {
            let mut inbox_messages = mailbox::read_messages(inbox_path)?;
            inbox_messages.push(envelope.clone());
            mailbox::store::commit_mailbox_state(inbox_path, &inbox_messages)?;
            Ok((
                (),
                workflow::remember_initial_state(workflow_state, envelope),
            ))
        },
    )
}

fn display_sender_identity(
    canonical_sender: &AgentName,
    sender_override: Option<&str>,
    sender_team: Option<&str>,
    recipient_team: &str,
    config: Option<&config::AtmConfig>,
) -> String {
    let cross_team = sender_team.is_some_and(|team| team != recipient_team);
    if !cross_team {
        return canonical_sender.to_string();
    }

    if let Some(sender_override) = sender_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        && config::aliases::resolve_agent(sender_override, config) == canonical_sender.as_str()
    {
        return sender_override.to_string();
    }

    config::aliases::preferred_alias(canonical_sender.as_str(), config)
        .unwrap_or_else(|| canonical_sender.to_string())
}

pub(super) fn qualified_sender_identity(sender: &AgentName, sender_team: Option<&str>) -> String {
    sender_team
        .map(|team| format!("{sender}@{team}"))
        .unwrap_or_else(|| sender.to_string())
}

fn set_canonical_sender_metadata(
    extra: &mut Map<String, serde_json::Value>,
    canonical_from: &AgentName,
) {
    let metadata = extra
        .entry("metadata".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !metadata.is_object() {
        *metadata = serde_json::Value::Object(Map::new());
    }
    let Some(metadata) = metadata.as_object_mut() else {
        return;
    };
    let atm = metadata
        .entry("atm".to_string())
        .or_insert_with(|| serde_json::Value::Object(Map::new()));
    if !atm.is_object() {
        *atm = serde_json::Value::Object(Map::new());
    }
    let Some(atm) = atm.as_object_mut() else {
        return;
    };
    atm.insert(
        "fromIdentity".to_string(),
        serde_json::to_value(canonical_from).expect("AgentName serializes"),
    );
}

pub(crate) fn maybe_run_post_send_hook(
    warnings: &mut Vec<String>,
    config: Option<&config::AtmConfig>,
    context: PostSendHookContext<'_>,
) {
    hook::maybe_run_post_send_hook(warnings, config, context);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;

    use super::alert_state;
    use crate::process::process_is_alive;
    use crate::send::{SendMessageSource, SendRequest};

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
            .insert("teams/atm-dev/config.json".to_string());

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
            Some("team-lead"),
            "../evil",
            Some("atm-dev"),
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
            Some("team-lead"),
            "arch-ctm",
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
}
