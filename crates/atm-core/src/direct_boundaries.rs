#![allow(
    dead_code,
    reason = "AC.2 keeps these internal compatibility helpers available while later consumer cutovers complete."
)]

use crate::boundary::{ConfigLoadRequest, ConfigLoadResponse};
use crate::error::AtmError;

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    crate::boundary_support::load_workspace_config(request)
}
