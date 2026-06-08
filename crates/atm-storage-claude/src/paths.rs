use std::path::{Path, PathBuf};

use atm_storage::{AgentName, AtmError, TeamName};

pub(crate) fn team_dir(home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
    Ok(home_dir.join(".claude").join("teams").join(team.as_str()))
}

pub(crate) fn inbox_path(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<PathBuf, AtmError> {
    Ok(team_dir(home_dir, team)?
        .join("inboxes")
        .join(format!("{agent}.json")))
}

pub(crate) fn team_config_path(home_dir: &Path, team: &TeamName) -> Result<PathBuf, AtmError> {
    Ok(team_dir(home_dir, team)?.join("config.json"))
}
