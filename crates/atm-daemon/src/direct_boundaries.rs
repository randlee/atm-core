use atm_core::{
    boundary::MessageFingerprint,
    boundary::{ConfigLoadRequest, ConfigLoadResponse},
    error::AtmError,
    load_atm_config,
};
use atm_storage::{AgentName, MessageEnvelope, TeamName};
use atm_storage_claude::compat::SourceFileRecord;
use std::path::Path;

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: load_atm_config(&request.current_dir)?,
    })
}

pub(crate) fn import_inbox_source(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFileRecord>, AtmError> {
    atm_storage_claude::compat::import_inbox_source(home_dir, team, agent)
}

pub(crate) fn compute_identity_fingerprint(
    message: &MessageEnvelope,
) -> Option<MessageFingerprint> {
    atm_storage_claude::compat::compute_identity_fingerprint(message).map(MessageFingerprint::from)
}
