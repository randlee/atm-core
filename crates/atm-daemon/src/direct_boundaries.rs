use atm_core::{
    boundary::MessageFingerprint,
    boundary::{ConfigLoadRequest, ConfigLoadResponse},
    error::AtmError,
    load_atm_config,
};
use atm_storage::{AgentName, MessageEnvelope, TeamName};
use std::path::Path;

use crate::claude_compat::SourceFileRecord;

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
    crate::claude_compat::import_inbox_source(home_dir, team, agent)
}

pub(crate) fn compute_identity_fingerprint(
    message: &MessageEnvelope,
) -> Option<MessageFingerprint> {
    crate::claude_compat::compute_identity_fingerprint(message).map(MessageFingerprint::from)
}
