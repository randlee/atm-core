use std::path::PathBuf;

use atm_core::error::AtmError;
use atm_daemon_client::{DaemonBinaryPath, DaemonLocalIpcEndpoint};

/// # Errors
///
/// Returns [`AtmError`] when the canonical same-host daemon socket path cannot
/// be resolved into a local IPC endpoint.
pub fn resolve_daemon_local_ipc_endpoint() -> Result<DaemonLocalIpcEndpoint, AtmError> {
    DaemonLocalIpcEndpoint::new(atm_core::protocol::daemon_socket_path()?)
}

/// # Errors
///
/// Returns [`AtmError`] when the current host executable path cannot be
/// resolved into the sibling `atm-daemon` binary path.
///
/// `ATM_DAEMON_BIN` is fully trusted process-owner input and intentionally
/// bypasses additional path validation.
pub fn resolve_daemon_bin(current_host_label: &str) -> Result<DaemonBinaryPath, AtmError> {
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
