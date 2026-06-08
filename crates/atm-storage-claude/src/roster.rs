use std::fs;
use std::path::Path;

use atm_storage::{
    AgentId, AgentName, AtmError, ModelName, PaneId, RosterHarness, RosterMember, RosterMemberKind,
    RosterSnapshot, TeamName,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredMember {
    name: AgentName,
    #[serde(default)]
    agent_id: AgentId,
    #[serde(default)]
    agent_type: atm_storage::contract::AgentType,
    #[serde(default)]
    model: ModelName,
    #[serde(default)]
    joined_at: Option<u64>,
    #[serde(default)]
    tmux_pane_id: Option<PaneId>,
    #[serde(default)]
    cwd: std::path::PathBuf,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct StoredTeamConfig {
    #[serde(default)]
    members: Vec<StoredMember>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn parse_team_config(path: &Path, raw: &str) -> Result<StoredTeamConfig, AtmError> {
    let root: Value = serde_json::from_str(raw).map_err(|error| {
        AtmError::config(format!(
            "failed to parse team config at {}: {error}",
            path.display()
        ))
        .with_source(error)
    })?;
    let object = root.as_object().ok_or_else(|| {
        AtmError::config(format!(
            "failed to parse team config at {}: root value must be a JSON object",
            path.display()
        ))
    })?;

    let members = match object.get("members") {
        None => Vec::new(),
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| match entry {
                Value::String(name) => name.parse::<AgentName>().ok().map(|name| StoredMember {
                    name,
                    agent_id: AgentId::default(),
                    agent_type: atm_storage::contract::AgentType::default(),
                    model: ModelName::default(),
                    joined_at: None,
                    tmux_pane_id: None,
                    cwd: std::path::PathBuf::new(),
                    extra: Map::new(),
                }),
                _ => serde_json::from_value::<StoredMember>(entry.clone()).ok(),
            })
            .collect(),
        Some(_) => {
            return Err(AtmError::config(format!(
                "failed to parse team config at {}: field 'members' must be a JSON array",
                path.display()
            )));
        }
    };

    let mut extra = object.clone();
    extra.remove("members");
    Ok(StoredTeamConfig { members, extra })
}

fn to_snapshot(team: &TeamName, config: StoredTeamConfig) -> RosterSnapshot {
    let members = config
        .members
        .into_iter()
        .map(|member| RosterMember {
            team_name: team.clone(),
            agent_name: member.name,
            member_kind: RosterMemberKind::Permanent,
            harness: RosterHarness::ClaudeCode,
            agent_type: member.agent_type,
            model: member.model,
            recipient_pane_id: member.tmux_pane_id,
            metadata_json: member.extra,
        })
        .collect();
    RosterSnapshot {
        team_name: team.clone(),
        members,
        refreshed_at: None,
    }
}

fn to_stored_config(roster: &RosterSnapshot) -> StoredTeamConfig {
    let members = roster
        .members
        .iter()
        .map(|member| StoredMember {
            name: member.agent_name.clone(),
            agent_id: AgentId::new(format!(
                "{}@{}",
                member.agent_name.as_str(),
                member.team_name.as_str()
            ))
            .expect("canonical roster member must produce a valid agent id"),
            agent_type: member.agent_type.clone(),
            model: member.model.clone(),
            joined_at: None,
            tmux_pane_id: member.recipient_pane_id.clone(),
            cwd: std::path::PathBuf::new(),
            extra: member.metadata_json.clone(),
        })
        .collect();
    StoredTeamConfig {
        members,
        extra: Map::new(),
    }
}

pub fn load_roster(home_dir: &Path, team: &TeamName) -> Result<RosterSnapshot, AtmError> {
    let path = crate::paths::team_config_path(home_dir, team);
    if !path.exists() {
        return Ok(RosterSnapshot {
            team_name: team.clone(),
            members: Vec::new(),
            refreshed_at: None,
        });
    }
    let raw = fs::read_to_string(&path).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read team config {}: {error}",
            path.display()
        ))
        .with_source(error)
    })?;
    parse_team_config(&path, &raw).map(|config| to_snapshot(team, config))
}

pub fn save_roster(home_dir: &Path, roster: &RosterSnapshot) -> Result<(), AtmError> {
    let path = crate::paths::team_config_path(home_dir, &roster.team_name);
    let parent = path.parent().ok_or_else(|| {
        AtmError::mailbox_write(format!("team config path {} has no parent", path.display()))
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to create team config directory {}: {error}",
            parent.display()
        ))
        .with_source(error)
    })?;
    let encoded = serde_json::to_vec_pretty(&to_stored_config(roster)).map_err(|error| {
        AtmError::mailbox_write("failed to encode team config").with_source(error)
    })?;
    fs::write(&path, encoded).map_err(|error| {
        AtmError::mailbox_write(format!(
            "failed to write team config {}: {error}",
            path.display()
        ))
        .with_source(error)
    })
}

pub fn list_teams(home_dir: &Path) -> Result<Vec<TeamName>, AtmError> {
    let teams_dir = home_dir.join(".claude").join("teams");
    if !teams_dir.exists() {
        return Ok(Vec::new());
    }
    let mut teams = Vec::new();
    for entry in fs::read_dir(&teams_dir).map_err(|error| {
        AtmError::mailbox_read(format!(
            "failed to read teams directory {}: {error}",
            teams_dir.display()
        ))
        .with_source(error)
    })? {
        let entry = entry.map_err(|error| {
            AtmError::mailbox_read(format!(
                "failed to enumerate teams directory {}: {error}",
                teams_dir.display()
            ))
            .with_source(error)
        })?;
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
            && let Some(name) = entry.file_name().to_str()
            && let Ok(team) = name.parse::<TeamName>()
        {
            teams.push(team);
        }
    }
    teams.sort();
    Ok(teams)
}
