// Windows owner-only ACL setup is the sole reviewed FFI exception; every
// other unsafe use remains a compile error.
#![deny(unsafe_code)]
//! Daemon runtime composition and portability adapters.

#[cfg_attr(not(windows), allow(dead_code))]
mod active_connection_registry;
#[cfg_attr(not(windows), allow(dead_code))]
mod daemon_runtime_observability;
mod daemon_worker_join;
mod ready_signal;
// ADR-002 (`docs/adr/ADR-002-host-wide-daemon-singleton.md`) intentionally splits
// launch.lock admission from owner.lock serving ownership so only one launcher can
// fork while only one daemon can publish the local IPC endpoint; see
// tests::host_ownership_record_uses_pid_and_token_while_held_and_clears_on_release.
mod host_ownership;
#[cfg_attr(windows, allow(dead_code))]
mod lifecycle_control;
#[cfg(not(windows))]
mod local_admission;
mod message_received_emitter;
#[allow(dead_code, reason = "AK.3 owns peer alias/resolver replacement")]
mod peer_resolution;
#[cfg_attr(windows, allow(dead_code))]
mod shutdown_beacon;
#[cfg(test)]
mod test_observability;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests_host_ownership;
#[cfg(test)]
mod tests_lifecycle;
#[cfg(test)]
mod worker_support;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use atm_core::error::{AtmError, AtmErrorCode};
pub use daemon_runtime_observability::{
    DaemonEvent, DaemonRuntimeObservability, DaemonSubsystem, TeamScope,
};

pub(crate) use daemon_runtime_observability::SubsystemObservability;
pub(crate) const GRACEFUL_DRAIN_DEADLINE: Duration = Duration::from_secs(2);
pub(crate) const FORCE_CANCEL_DEADLINE: Duration = Duration::from_secs(3);
/// Shared local HTTP connection bound for both Unix UDS and loopback TCP.
pub(crate) const MAX_KEEP_ALIVE_REQUESTS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaemonExitCode {
    CleanStop = 0,
    InternalBug = 1,
    DoNotRestart = 64,
    TransportFatal = 70,
    LifecycleWedge = 71,
}

impl DaemonExitCode {
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

pub fn daemon_exit_code_for_error(error: &AtmError) -> DaemonExitCode {
    if error.code() == AtmErrorCode::DaemonLifecycleWedge {
        return DaemonExitCode::LifecycleWedge;
    }
    if matches!(
        error.code(),
        AtmErrorCode::DaemonServingStateRejected
            | AtmErrorCode::DaemonStaleOwnerRecoveryFailed
            | AtmErrorCode::DaemonLaunchGateRejected
            | AtmErrorCode::ConfigHomeUnavailable
            | AtmErrorCode::ObservabilityBootstrapFailed
    ) || matches!(
        error.code(),
        AtmErrorCode::ConfigParseFailed
            | AtmErrorCode::ConfigRetiredHookMembersKey
            | AtmErrorCode::ConfigRetiredLegacyHookKeys
            | AtmErrorCode::ConfigTeamParseFailed
            | AtmErrorCode::ConfigTeamMissing
    ) {
        return DaemonExitCode::DoNotRestart;
    }
    if error.code() == AtmErrorCode::DaemonUnavailable {
        return DaemonExitCode::TransportFatal;
    }
    DaemonExitCode::InternalBug
}

#[derive(Debug, Clone)]
pub(crate) struct AtmHomeDir(PathBuf);

impl AtmHomeDir {
    pub(crate) fn resolve() -> Result<Self, AtmError> {
        Ok(Self(atm_core::home::atm_home()?))
    }

    pub(crate) fn as_path(&self) -> &std::path::Path {
        &self.0
    }

    #[cfg(test)]
    pub(crate) fn from_path_for_test(path: PathBuf) -> Self {
        Self(path)
    }
}
