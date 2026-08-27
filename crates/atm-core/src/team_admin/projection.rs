use std::path::Path;

use serde_json::Value;

use crate::boundary::{RosterEntry, RosterStore};
use crate::delivery_channel::local_message_received_backend;
use crate::error::AtmError;
use crate::roles::ROLE_TEAM_LEAD;
use crate::schema::agent_member::LEGACY_CWD_METADATA_KEY;
#[cfg(test)]
use crate::schema::{AgentMember, TeamConfig};
use crate::schema::{HOME_DIR_METADATA_KEY, canonical_home_dir};
#[cfg(test)]
use crate::types::AgentId;
use crate::types::AgentName;

use super::{MemberSummary, MembersList, MembersQuery};

pub(super) fn list_members_from_roster_store(
    roster_store: &dyn RosterStore,
    query: MembersQuery,
) -> Result<MembersList, AtmError> {
    let roster = load_team_roster(roster_store, &query.team)?;
    if roster.is_empty() {
        return Err(AtmError::team_not_found(&query.team));
    }

    Ok(MembersList {
        team: query.team,
        members: ordered_roster_member_summaries(
            &roster,
            query.caller_identity.as_ref(),
            query.live_cwd.as_deref(),
        ),
    })
}

pub(super) fn load_team_roster(
    roster_store: &dyn RosterStore,
    team: &crate::types::TeamName,
) -> Result<Vec<RosterEntry>, AtmError> {
    roster_store.load_roster(team)
}

#[cfg(test)]
pub(super) fn project_team_config_from_roster(
    extra: serde_json::Map<String, Value>,
    records: &[RosterEntry],
) -> Result<TeamConfig, AtmError> {
    let mut members = Vec::with_capacity(records.len());
    if let Some(team_lead) = records
        .iter()
        .find(|member| member.agent_name == ROLE_TEAM_LEAD)
    {
        members.push(agent_member_from_roster_record(team_lead)?);
    }
    for record in records {
        if record.agent_name == ROLE_TEAM_LEAD {
            continue;
        }
        members.push(agent_member_from_roster_record(record)?);
    }
    Ok(TeamConfig { members, extra })
}

pub(super) fn ordered_roster_member_summaries(
    records: &[RosterEntry],
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> Vec<MemberSummary> {
    let mut members = Vec::with_capacity(records.len());
    if let Some(team_lead) = records
        .iter()
        .find(|member| member.agent_name == ROLE_TEAM_LEAD)
    {
        members.push(member_summary_from_roster(
            team_lead,
            caller_identity,
            live_cwd,
        ));
    }
    for record in records {
        if record.agent_name == ROLE_TEAM_LEAD {
            continue;
        }
        members.push(member_summary_from_roster(
            record,
            caller_identity,
            live_cwd,
        ));
    }
    members
}

fn member_summary_from_roster(
    record: &RosterEntry,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> MemberSummary {
    let local_backend = local_message_received_backend(record);
    let (backend, herdr_session) = match local_backend.as_ref() {
        Some(crate::delivery_channel::LocalMessageReceivedBackend::Herdr { session }) => (
            Some("herdr".to_string()),
            session.as_ref().map(ToString::to_string),
        ),
        _ => (None, None),
    };
    MemberSummary {
        name: record.agent_name.clone(),
        agent_id: metadata_string(&record.metadata_json, "agentId")
            .unwrap_or_else(|| format!("{}@{}", record.agent_name, record.team_name)),
        agent_type: record.agent_type.to_string(),
        harness: record.harness,
        model: record.model.clone(),
        joined_at: metadata_u64(&record.metadata_json, "joinedAt"),
        tmux_pane_id: record.recipient_pane_id.clone(),
        backend,
        herdr_session,
        local_backend,
        home_dir: canonical_home_dir(&record.metadata_json).unwrap_or_default(),
        live_cwd: runtime_live_cwd(record, caller_identity, live_cwd),
        host: member_registered_host(record),
        extra: compatibility_extra_fields(&record.metadata_json),
    }
}

/// Reads this member's registered host (ADR-055 decision (e)) from roster
/// metadata for display/projection. Lenient by design: a malformed stored
/// value degrades to `None` here rather than failing the whole roster
/// listing -- `crate::send_to::member_host` is the fail-closed counterpart
/// used by `--from-json` routing, where a malformed host must be reported.
fn member_registered_host(record: &RosterEntry) -> Option<crate::types::HostName> {
    record
        .metadata_json
        .get(crate::send_to::ROSTER_HOST_METADATA_KEY)
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse().ok())
}

#[cfg(test)]
fn agent_member_from_roster_record(record: &RosterEntry) -> Result<AgentMember, AtmError> {
    let mut extra = compatibility_extra_fields(&record.metadata_json);
    Ok(AgentMember {
        name: record.agent_name.clone(),
        agent_id: roster_record_agent_id(record)?,
        agent_type: record.agent_type.clone(),
        model: record.model.clone(),
        joined_at: metadata_u64(&record.metadata_json, "joinedAt"),
        tmux_pane_id: record.recipient_pane_id.clone(),
        home_dir: canonical_home_dir(&record.metadata_json).unwrap_or_default(),
        extra: {
            extra.remove("agentId");
            extra.remove("joinedAt");
            extra.remove(HOME_DIR_METADATA_KEY);
            #[allow(
                deprecated,
                reason = "Phase AD obsolete: derived compatibility field only"
            )]
            extra.remove(LEGACY_CWD_METADATA_KEY);
            extra
        },
    })
}

#[cfg(test)]
fn roster_record_agent_id(record: &RosterEntry) -> Result<AgentId, AtmError> {
    let raw_agent_id = metadata_string(&record.metadata_json, "agentId")
        .unwrap_or_else(|| format!("{}@{}", record.agent_name, record.team_name));
    AgentId::new(raw_agent_id.clone()).map_err(|error| {
        AtmError::validation(format!(
            "roster member {}@{} has invalid persisted agentId '{}': {error}",
            record.agent_name, record.team_name, raw_agent_id
        ))
    })
}

fn compatibility_extra_fields(
    metadata_json: &serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut extra = metadata_json.clone();
    extra.remove("agentId");
    extra.remove("joinedAt");
    extra.remove(HOME_DIR_METADATA_KEY);
    #[allow(
        deprecated,
        reason = "Phase AD obsolete: derived compatibility field only"
    )]
    extra.remove(LEGACY_CWD_METADATA_KEY);
    extra
}

fn runtime_live_cwd(
    record: &RosterEntry,
    caller_identity: Option<&AgentName>,
    live_cwd: Option<&Path>,
) -> Option<String> {
    match (caller_identity, live_cwd) {
        (Some(identity), Some(path)) if *identity == record.agent_name => {
            Some(path.display().to_string())
        }
        _ => None,
    }
}

fn metadata_string(metadata_json: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    metadata_json
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn metadata_u64(metadata_json: &serde_json::Map<String, Value>, key: &str) -> Option<u64> {
    metadata_json.get(key).and_then(Value::as_u64)
}
