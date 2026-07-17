#[cfg(test)]
use atm_core::boundary::MessageFingerprint;
#[cfg(test)]
use atm_core::error::AtmError;
#[cfg(test)]
use atm_storage::{AgentName, MessageEnvelope, TeamName};
#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use crate::claude_compat::SourceFileRecord;

#[cfg(test)]
pub(crate) fn import_inbox_source(
    home_dir: &Path,
    team: &TeamName,
    agent: &AgentName,
) -> Result<Vec<SourceFileRecord>, AtmError> {
    crate::claude_compat::import_inbox_source(home_dir, team, agent)
}

#[cfg(test)]
pub(crate) fn compute_identity_fingerprint(
    message: &MessageEnvelope,
) -> Option<MessageFingerprint> {
    crate::claude_compat::compute_identity_fingerprint(message).map(MessageFingerprint::from)
}
