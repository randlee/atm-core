use std::path::{Path, PathBuf};

use atm_storage::{AgentName, AtmError, TeamName};

fn validate_segment(value: &str, kind: &str) -> Result<(), AtmError> {
    if value.is_empty() {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not be empty"
        )));
    }
    if value.starts_with('.') {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not start with '.'"
        )));
    }
    if value.contains("..") {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain '..'"
        )));
    }
    if value.contains(['/', '\\']) {
        return Err(AtmError::address_parse(format!(
            "{kind} name must not contain path separators"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(AtmError::address_parse(format!(
            "{kind} name contains invalid characters"
        )));
    }
    Ok(())
}

pub fn team_dir(home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
    validate_segment(team.as_str(), "team")?;
    Ok(home_dir.join(".claude").join("teams").join(team.as_str()))
}

pub fn inbox_path(home_dir: &Path, team: &TeamName, agent: &AgentName) -> Result<PathBuf, AtmError> {
    validate_segment(agent.as_str(), "agent")?;
    Ok(team_dir(home_dir, team)?
        .join("inboxes")
        .join(format!("{agent}.json")))
}

pub fn team_config_path(home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
    Ok(team_dir(home_dir, team)?.join("config.json"))
}
