use std::fs;
use std::path::{Path, PathBuf};

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
use crate::read::state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::send::{input, summary};
use crate::types::{IsoTimestamp, MessageClass};

#[derive(Debug, Clone)]
pub struct AckRequest {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<String>,
    pub team_override: Option<String>,
    pub message_id: Uuid,
    pub reply_body: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AckOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub message_id: Uuid,
    pub reply_target: String,
    pub reply_message_id: Uuid,
}

pub fn ack_mail(
    request: AckRequest,
    observability: &dyn ObservabilityPort,
) -> Result<AckOutcome, AtmError> {
    let config = config::load_config(&request.current_dir)?;
    let actor = resolve_actor_identity(request.actor_override.as_deref(), config.as_ref())?;
    let team = config::resolve_team(request.team_override.as_deref(), config.as_ref())
        .ok_or_else(AtmError::team_unavailable)?;
    let team_dir = home::team_dir_from_home(&request.home_dir, &team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&team));
    }

    let team_config = load_team_config(&team_dir)?;
    if !team_config
        .members
        .iter()
        .any(|member| member.name == actor)
    {
        return Err(AtmError::agent_not_found(&actor, &team));
    }

    let inbox_path = home::inbox_path_from_home(&request.home_dir, &team, &actor)?;
    let mut source_messages = mailbox::read_messages(&inbox_path)?;
    let source_messages_original = source_messages.clone();
    let source_index = source_messages
        .iter()
        .position(|message| message.message_id == Some(request.message_id))
        .ok_or_else(|| {
            AtmError::validation(format!(
                "message {} was not found in {}@{}",
                request.message_id, actor, team
            ))
        })?;

    let source_message = &source_messages[source_index];
    match state::classify_message(source_message) {
        MessageClass::PendingAck => {}
        MessageClass::Acknowledged => {
            return Err(AtmError::validation(format!(
                "message {} is already acknowledged",
                request.message_id
            )));
        }
        _ => {
            return Err(AtmError::validation(format!(
                "message {} is not pending acknowledgement",
                request.message_id
            )));
        }
    }

    let (reply_agent, reply_team) = resolve_reply_target(source_message, &team)?;
    let reply_team_dir = home::team_dir_from_home(&request.home_dir, &reply_team)?;
    if !reply_team_dir.exists() {
        return Err(AtmError::team_not_found(&reply_team));
    }

    let reply_team_config = load_team_config(&reply_team_dir)?;
    if !reply_team_config
        .members
        .iter()
        .any(|member| member.name == reply_agent)
    {
        return Err(AtmError::agent_not_found(&reply_agent, &reply_team));
    }

    let ack_timestamp = IsoTimestamp::now();
    let reply_text = input::validate_message_text(request.reply_body)?;
    let reply_message_id = Uuid::new_v4();
    let reply_message = MessageEnvelope {
        from: actor.clone(),
        text: reply_text.clone(),
        timestamp: ack_timestamp.into_inner(),
        read: false,
        source_team: Some(team.clone()),
        summary: Some(summary::build_summary(&reply_text, None)),
        message_id: Some(reply_message_id),
        pending_ack_at: None,
        acknowledged_at: None,
        acknowledges_message_id: Some(request.message_id),
        task_id: None,
        extra: Map::new(),
    };

    {
        let source = &mut source_messages[source_index];
        source.read = true;
        source.pending_ack_at = None;
        source.acknowledged_at = Some(ack_timestamp.into_inner());
    }

    let reply_inbox_path =
        home::inbox_path_from_home(&request.home_dir, &reply_team, &reply_agent)?;
    if reply_inbox_path == inbox_path {
        source_messages.push(reply_message);
        mailbox::atomic::write_messages(&inbox_path, &source_messages)?;
    } else {
        let reply_messages_original = mailbox::read_messages(&reply_inbox_path)?;
        let mut reply_messages = reply_messages_original.clone();
        reply_messages.push(reply_message);

        mailbox::atomic::write_messages(&inbox_path, &source_messages)?;
        if let Err(error) = mailbox::atomic::write_messages(&reply_inbox_path, &reply_messages) {
            let _ = mailbox::atomic::write_messages(&inbox_path, &source_messages_original);
            let _ = mailbox::atomic::write_messages(&reply_inbox_path, &reply_messages_original);
            return Err(error);
        }
    }

    let outcome = AckOutcome {
        action: "ack",
        team: team.clone(),
        agent: actor.clone(),
        message_id: request.message_id,
        reply_target: format!("{reply_agent}@{reply_team}"),
        reply_message_id,
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "ack",
        action: "ack",
        outcome: "ok",
        team,
        agent: actor.clone(),
        sender: actor,
        message_id: request.message_id.to_string(),
        requires_ack: false,
        dry_run: false,
        task_id: None,
    });

    Ok(outcome)
}

fn resolve_actor_identity(
    actor_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<String, AtmError> {
    if let Some(actor) = actor_override.filter(|value| !value.trim().is_empty()) {
        return Ok(actor.to_string());
    }

    if let Some(identity) = identity::hook::read_hook_identity()? {
        return Ok(identity);
    }

    identity::resolve_sender_identity(config)
}

fn resolve_reply_target(
    message: &MessageEnvelope,
    current_team: &str,
) -> Result<(String, String), AtmError> {
    let parsed: AgentAddress = if message.from.contains('@') {
        message.from.parse()?
    } else {
        AgentAddress {
            agent: message.from.clone(),
            team: message
                .source_team
                .clone()
                .or_else(|| Some(current_team.to_string())),
        }
    };

    let team = parsed.team.ok_or_else(AtmError::team_unavailable)?;
    Ok((parsed.agent, team))
}

fn load_team_config(team_dir: &Path) -> Result<TeamConfig, AtmError> {
    let config_path = team_dir.join("config.json");
    let raw = fs::read_to_string(&config_path).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "failed to read team config at {}: {error}",
                config_path.display()
            ),
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
