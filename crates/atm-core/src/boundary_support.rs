#![allow(
    dead_code,
    reason = "AC.2 preserves these crate-private Claude compatibility helpers until later internal consumers are removed."
)]

//! Hidden config-ingress helper layer used by concrete boundary adapters.

use crate::boundary::{ConfigLoadRequest, ConfigLoadResponse};
use crate::config;
use crate::error::AtmError;

pub(crate) fn load_workspace_config(
    request: ConfigLoadRequest,
) -> Result<ConfigLoadResponse, AtmError> {
    Ok(ConfigLoadResponse {
        config: config::load_config(&request.current_dir).map_err(|_error| {
            AtmError::config(format!(
                "daemon ConfigIngress could not load workspace config from {}",
                request.current_dir.display()
            ))
        })?,
    })
}
