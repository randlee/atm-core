use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{TimeDelta, Utc};
use serde::Serialize;
use serde_json::Value;

use crate::address::AgentAddress;
use crate::config;
use crate::error::{AtmError, AtmErrorKind};
use crate::home;
use crate::identity;
use crate::mailbox;
use crate::observability::{CommandEvent, ObservabilityPort};
use crate::read::state;
use crate::schema::{MessageEnvelope, TeamConfig};
use crate::types::MessageClass;

#[derive(Debug, Clone)]
pub struct ClearQuery {
    pub home_dir: PathBuf,
    pub current_dir: PathBuf,
    pub actor_override: Option<String>,
    pub target_address: Option<String>,
    pub team_override: Option<String>,
    pub older_than: Option<Duration>,
    pub idle_only: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RemovedByClass {
    pub unread: usize,
    pub pending_ack: usize,
    pub acknowledged: usize,
    pub read: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearOutcome {
    pub action: &'static str,
    pub team: String,
    pub agent: String,
    pub removed_total: usize,
    pub remaining_total: usize,
    pub removed_by_class: RemovedByClass,
}

pub fn clear_mail(
    query: ClearQuery,
    observability: &dyn ObservabilityPort,
) -> Result<ClearOutcome, AtmError> {
    let config = config::load_config(&query.current_dir)?;
    let actor = resolve_actor_identity(query.actor_override.as_deref(), config.as_ref())?;
    let target = resolve_target(
        query.target_address.as_deref(),
        &actor,
        query.team_override.as_deref(),
        config.as_ref(),
    )?;

    let team_dir = home::team_dir_from_home(&query.home_dir, &target.team)?;
    if !team_dir.exists() {
        return Err(AtmError::team_not_found(&target.team));
    }

    let team_config = load_team_config(&team_dir)?;
    if target.explicit
        && !team_config
            .members
            .iter()
            .any(|member| member.name == target.agent)
    {
        return Err(AtmError::agent_not_found(&target.agent, &target.team));
    }

    let inbox_path = home::inbox_path_from_home(&query.home_dir, &target.team, &target.agent)?;
    let messages = mailbox::read_messages(&inbox_path)?;
    let cutoff = cutoff_timestamp(query.older_than)?;

    let mut kept = Vec::with_capacity(messages.len());
    let mut removed_by_class = RemovedByClass::default();

    for message in messages {
        let class = state::classify_message(&message);
        let clearable = matches!(class, MessageClass::Read | MessageClass::Acknowledged)
            && cutoff
                .map(|cutoff| message.timestamp <= cutoff)
                .unwrap_or(true)
            && (!query.idle_only || is_idle_notification(&message));

        if clearable {
            count_removed(&mut removed_by_class, class);
        } else {
            kept.push(message);
        }
    }

    if !query.dry_run {
        mailbox::atomic::write_messages(&inbox_path, &kept)?;
    }

    let outcome = ClearOutcome {
        action: "clear",
        team: target.team,
        agent: target.agent,
        removed_total: removed_total(&removed_by_class),
        remaining_total: kept.len(),
        removed_by_class,
    };

    let _ = observability.emit_command_event(CommandEvent {
        command: "clear",
        action: "clear",
        outcome: if query.dry_run { "dry_run" } else { "ok" },
        team: outcome.team.clone(),
        agent: outcome.agent.clone(),
        sender: actor,
        message_id: String::new(),
        requires_ack: false,
        dry_run: query.dry_run,
        task_id: None,
    });

    Ok(outcome)
}

#[derive(Debug)]
struct ResolvedTarget {
    agent: String,
    team: String,
    explicit: bool,
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

fn resolve_target(
    target_address: Option<&str>,
    actor: &str,
    team_override: Option<&str>,
    config: Option<&config::AtmConfig>,
) -> Result<ResolvedTarget, AtmError> {
    let Some(target_address) = target_address else {
        let team =
            config::resolve_team(team_override, config).ok_or_else(AtmError::team_unavailable)?;
        return Ok(ResolvedTarget {
            agent: actor.to_string(),
            team,
            explicit: false,
        });
    };

    let parsed: AgentAddress = target_address.parse()?;
    let team = parsed
        .team
        .or_else(|| config::resolve_team(team_override, config))
        .ok_or_else(AtmError::team_unavailable)?;

    Ok(ResolvedTarget {
        agent: parsed.agent,
        team,
        explicit: true,
    })
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

fn cutoff_timestamp(
    older_than: Option<Duration>,
) -> Result<Option<chrono::DateTime<Utc>>, AtmError> {
    older_than
        .map(|duration| {
            TimeDelta::from_std(duration)
                .map_err(|error| AtmError::validation(format!("invalid duration filter: {error}")))
        })
        .transpose()
        .map(|delta| delta.map(|delta| Utc::now() - delta))
}

fn is_idle_notification(message: &MessageEnvelope) -> bool {
    serde_json::from_str::<Value>(&message.text)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
        .as_deref()
        == Some("idle_notification")
}

fn count_removed(counts: &mut RemovedByClass, class: MessageClass) {
    match class {
        MessageClass::Unread => counts.unread += 1,
        MessageClass::PendingAck => counts.pending_ack += 1,
        MessageClass::Acknowledged => counts.acknowledged += 1,
        MessageClass::Read => counts.read += 1,
    }
}

fn removed_total(counts: &RemovedByClass) -> usize {
    counts.unread + counts.pending_ack + counts.acknowledged + counts.read
}
