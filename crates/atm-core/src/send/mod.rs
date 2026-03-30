use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use serde_json::Map;
use uuid::Uuid;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::schema::{MessageEnvelope, TeamConfig};

pub mod file_policy;
pub mod input;
pub mod summary;

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
    pub sender_override: Option<String>,
    pub to: String,
    pub team_override: Option<String>,
    pub message_source: SendMessageSource,
    pub summary_override: Option<String>,
    pub requires_ack: bool,
    pub task_id: Option<String>,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SendOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub sender: String,
    pub outcome: &'static str,
    pub message_id: Uuid,
    pub requires_ack: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub dry_run: bool,
}

pub fn send_mail(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    let sender = resolve_sender_identity(request.sender_override.as_deref(), config.as_ref())?;
    let recipient = resolve_recipient(
        &request.to,
        request.team_override.as_deref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir_from_home(&request.home_dir, &recipient.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&recipient.team));
    }

    let team_config = load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == recipient.agent)
    {
        return Err(AtmError::agent_not_found(&recipient.agent, &recipient.team));
    }

    let task_id = input::validate_task_id(request.task_id)?;
    let requires_ack = request.requires_ack || task_id.is_some();
    let body = resolve_message_body(
        &request.message_source,
        &request.current_dir,
        &request.home_dir,
        &recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override);
    let message_id = Uuid::new_v4();
    let timestamp = Utc::now();

    if !request.dry_run {
        let envelope = MessageEnvelope {
            from: sender.clone(),
            text: body.clone(),
            timestamp,
            read: false,
            source_team: Some(recipient.team.clone()),
            summary: Some(summary.clone()),
            message_id: Some(message_id),
            pending_ack_at: requires_ack.then_some(timestamp),
            acknowledged_at: None,
            acknowledges_message_id: None,
            extra: Map::new(),
        };
        let inbox_path =
            home::inbox_path_from_home(&request.home_dir, &recipient.team, &recipient.agent)?;
        mailbox::append_message(&inbox_path, &envelope)?;
    }

    let outcome = SendOutcome {
        action: "send",
        team: recipient.team.clone(),
        agent: recipient.agent.clone(),
        sender: sender.clone(),
        outcome: if request.dry_run { "dry_run" } else { "sent" },
        message_id,
        requires_ack,
        task_id: task_id.clone(),
        summary: Some(summary),
        message: request.dry_run.then_some(body.clone()),
        dry_run: request.dry_run,
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "send",
        action: "send",
        outcome: outcome.outcome,
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender,
        message_id: outcome.message_id.to_string(),
        requires_ack: outcome.requires_ack,
        dry_run: outcome.dry_run,
        task_id,
    });

    Ok(outcome)
}

#[derive(Debug)]
struct ResolvedRecipient {
    agent: String,
    team: String,
}

fn resolve_sender_identity(
    sender_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<String, AtmError> {
    if let Some(sender) = sender_override.filter(|value| !value.trim().is_empty()) {
        return Ok(sender.to_string());
    }

    if let Some(identity) = identity::hook::read_hook_identity()? {
        return Ok(identity);
    }

    identity::resolve_sender_identity(config)
}

fn resolve_recipient(
    target_address: &str,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedRecipient, AtmError> {
    let parsed: AgentAddress = target_address.parse()?;
    let team = parsed
        .team
        .or_else(|| config::resolve_team(team_override, config))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedRecipient {
        agent: parsed.agent,
        team,
    })
}

fn load_team_config(team_dir: &Path) -> Result<TeamConfig, AtmError> {
    let config_path = team_dir.join("config.json");
    let raw = fs::read_to_string(&config_path).map_err(|error| {
        AtmError::team_not_found(
            team_dir
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("unknown"),
        )
        .with_source(error)
    })?;

    serde_json::from_str(&raw).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to parse team config at {}: {error}",
                config_path.display()
            ),
        )
        .with_source(error)
    })
}

fn resolve_message_body(
    source: &SendMessageSource,
    current_dir: &Path,
    home_dir: &Path,
    team_name: &str,
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
