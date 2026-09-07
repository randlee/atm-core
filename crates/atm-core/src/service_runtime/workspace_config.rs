use std::path::Path;

use crate::config::{self, AtmConfig};
use crate::error::AtmError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceConfigAccess {
    #[default]
    Client,
    Disabled,
}

pub(super) fn load_workspace_config(
    access: WorkspaceConfigAccess,
    current_dir: &Path,
) -> Result<Option<AtmConfig>, AtmError> {
    match access {
        WorkspaceConfigAccess::Client => config::load_config(current_dir),
        WorkspaceConfigAccess::Disabled => Ok(None),
    }
}
