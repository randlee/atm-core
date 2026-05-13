use std::path::PathBuf;

use atm_core::error::AtmError;
use atm_daemon_client::{DaemonBinaryPath, DaemonLocalIpcEndpoint};

pub(crate) fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(atm_core::protocol::daemon_socket_path()?)
}

pub(crate) fn resolve_daemon_bin(current_host_label: &str) -> Result<DaemonBinaryPath, AtmError> {
    if let Some(path) = std::env::var_os("ATM_DAEMON_BIN").filter(|value| !value.is_empty()) {
        return DaemonBinaryPath::new(PathBuf::from(path));
    }
    let current = std::env::current_exe().map_err(|source| {
        AtmError::daemon_unavailable(format!(
            "failed to resolve the current {current_host_label} executable path"
        ))
        .with_source(source)
    })?;
    DaemonBinaryPath::new(
        current.with_file_name(format!("atm-daemon{}", std::env::consts::EXE_SUFFIX)),
    )
}
