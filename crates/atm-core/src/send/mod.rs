use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::address::AgentAddress;
use crate::config;
use crate::error::AtmError;
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
    InlineText(String),
    StdinText(String),
    FileReference(PathBuf),
}

#[derive(Debug, Clone)]
pub struct SendRequest {
    pub current_dir: PathBuf,
    pub sender_override: Option<String>,
    pub target_address: String,
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

pub fn execute(
    request: SendRequest,
    observability: &dyn ObservabilityPort,
) -> Result<SendOutcome, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    let sender = resolve_sender_identity(request.sender_override.as_deref(), config.as_ref())?;
    let recipient = resolve_recipient(
        &request.target_address,
        request.team_override.as_deref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir(&recipient.team)?;
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
        &recipient.team,
    )?;
    let summary = summary::build_summary(&body, request.summary_override);
    let message_id = Uuid::new_v4();

    if !request.dry_run {
        let envelope = MessageEnvelope {
            message_id,
            from: sender.clone(),
            team: recipient.team.clone(),
            body: body.clone(),
            requires_ack,
            task_id: task_id.clone(),
            sent_at: Utc::now(),
        };
        let inbox_path = home::inbox_path(&recipient.team, &recipient.agent)?;
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
    match sender_override.filter(|value| !value.trim().is_empty()) {
        Some(sender) => Ok(sender.to_string()),
        None => identity::resolve_sender_identity(config),
    }
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
            crate::error::AtmErrorKind::Config,
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
    team_name: &str,
) -> Result<String, AtmError> {
    match source {
        SendMessageSource::InlineText(message) | SendMessageSource::StdinText(message) => {
            input::validate_message_text(message.clone())
        }
        SendMessageSource::FileReference(path) => {
            file_policy::process_file_reference(path, None, team_name, current_dir)
        }
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}
