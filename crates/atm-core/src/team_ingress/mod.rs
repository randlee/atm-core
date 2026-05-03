use std::collections::BTreeMap;
use std::path::Path;

use crate::config::load_team_config;
use crate::error::{AtmError, AtmErrorKind};
use crate::roster_store::{RosterMemberRecord, RosterStore, TransportKind};
use crate::schema::{AgentMember, AgentType};
use crate::store::{HostName, RecipientPaneId, StoreError};
use crate::types::TeamName;

const DEFAULT_ROLE: &str = "member";
const DEFAULT_TRANSPORT_KIND: &str = "claude-code";

pub fn default_host_name() -> HostName {
    std::env::var("ATM_HOST")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| "local-host".parse().expect("local-host is valid"))
}

pub fn ingest_team_config(
    team_dir: &Path,
    team_name: &TeamName,
    roster_store: &impl RosterStore,
    host_name: &HostName,
) -> Result<Vec<RosterMemberRecord>, AtmError> {
    let team_config = load_team_config(team_dir)?;
    let existing = roster_store
        .load_roster(team_name)
        .map_err(|error| map_store_error("failed to load existing roster rows", error))?;
    let existing_by_name = existing
        .into_iter()
        .map(|member| (member.agent_name.clone(), member))
        .collect::<BTreeMap<_, _>>();

    let mut roster = Vec::with_capacity(team_config.members.len());
    for member in &team_config.members {
        roster.push(roster_record_for_member(
            member,
            team_name,
            host_name,
            existing_by_name.get(&member.name),
        )?);
    }

    roster_store
        .replace_roster(team_name, &roster)
        .map_err(|error| map_store_error("failed to replace roster from config.json", error))?;
    Ok(roster)
}

fn roster_record_for_member(
    member: &AgentMember,
    team_name: &TeamName,
    host_name: &HostName,
    existing: Option<&RosterMemberRecord>,
) -> Result<RosterMemberRecord, AtmError> {
    let role = match &member.agent_type {
        AgentType::Unknown(raw) if raw.trim().is_empty() => DEFAULT_ROLE.parse(),
        agent_type => agent_type.to_string().parse(),
    }
    .map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "invalid roster role derived from config.json member {}: {error}",
                member.name
            ),
        )
        .with_recovery(
            "Repair the config.json agentType field or use a non-empty compatibility value before retrying.",
        )
    })?;
    let transport_kind = DEFAULT_TRANSPORT_KIND
        .parse::<TransportKind>()
        .map_err(|error| {
            AtmError::new(
                AtmErrorKind::Config,
                format!("invalid built-in transport kind {DEFAULT_TRANSPORT_KIND}: {error}"),
            )
        })?;
    let recipient_pane_id = pane_id_for_member(member)?;
    let metadata_json = serde_json::to_string(member).map_err(|source| {
        AtmError::new(
            AtmErrorKind::Serialization,
            format!(
                "failed to encode config.json member {} into roster metadata",
                member.name
            ),
        )
        .with_recovery(
            "Repair unsupported roster metadata values in config.json before retrying roster ingest.",
        )
        .with_source(source)
    })?;

    Ok(RosterMemberRecord {
        team_name: team_name.clone(),
        agent_name: member.name.clone(),
        role,
        transport_kind,
        host_name: host_name.clone(),
        recipient_pane_id,
        pid: existing.and_then(|member| member.pid),
        metadata_json: Some(metadata_json),
    })
}

fn pane_id_for_member(member: &AgentMember) -> Result<Option<RecipientPaneId>, AtmError> {
    let value = member.tmux_pane_id.trim();
    if value.is_empty() {
        return Ok(None);
    }
    value.parse().map(Some).map_err(|error| {
        AtmError::new(
            AtmErrorKind::Config,
            format!(
                "invalid tmuxPaneId `{value}` for config.json member {}: {error}",
                member.name
            ),
        )
        .with_recovery(
            "Repair the tmuxPaneId field in config.json so ATM can persist an authoritative pane mapping.",
        )
    })
}

fn map_store_error(context: &str, error: StoreError) -> AtmError {
    let mut atm_error = AtmError::new_with_code(
        error.code,
        AtmErrorKind::MailboxWrite,
        format!("{context}: {}", error.message),
    );
    if let Some(recovery) = error.recovery.as_ref() {
        atm_error = atm_error.with_recovery(recovery.clone());
    }
    atm_error.with_source(error)
}
