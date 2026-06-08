use atm_core::{
    boundary::MessageFingerprint,
    boundary::{ConfigLoadRequest, ConfigLoadResponse},
    error::AtmError,
    load_atm_config,
};
use atm_storage::{AgentName, MessageEnvelope, TeamName};
use atm_storage_claude::compat::{
    SourceFileRecord, SourceIngressIdentityFingerprintRequest, SourceIngressImportRequest,
};
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
    atm_storage_claude::compat::import_inbox_source(SourceIngressImportRequest {
        home_dir: home_dir.to_path_buf(),
        team: team.clone(),
        agent: agent.clone(),
    })
    .map(|response| response.source_files)
}

pub(crate) fn compute_identity_fingerprint(
    message: &MessageEnvelope,
) -> Option<MessageFingerprint> {
    atm_storage_claude::compat::compute_identity_fingerprint(
        SourceIngressIdentityFingerprintRequest {
            message: message.clone(),
        },
    )
    .fingerprint
    .map(MessageFingerprint::from)
}
