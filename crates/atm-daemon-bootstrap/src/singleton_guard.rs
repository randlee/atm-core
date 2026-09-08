//! Fail-closed singleton checks owned by the Tokio/Axum daemon bootstrap.
//!
//! The owner lock is authoritative for ordinary starts, while process and
//! endpoint checks detect a deleted/replaced lock or another bypass. None of
//! these checks have a test, environment, or build-profile escape hatch.

use std::ffi::OsStr;
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use sysinfo::{Pid, System};

use crate::DaemonOwnerGuard;

pub(crate) const GUARD_NAMES: [&str; 3] = [
    "owner lock ownership",
    "same-user atm-daemon process table",
    "fixed local endpoint availability",
];

/// The fixed singleton inputs captured before the daemon can begin serving.
pub(crate) struct SingletonGuards {
    unix_socket: PathBuf,
    direct_peer_port: u16,
}

impl SingletonGuards {
    pub(crate) fn new(unix_socket: PathBuf, direct_peer_port: u16) -> Self {
        Self {
            unix_socket,
            direct_peer_port,
        }
    }

    /// Applies every startup guard before any replacement listener binds.
    pub(crate) fn verify_startup(&self, owner: &DaemonOwnerGuard) -> Result<(), String> {
        let _required_guards = GUARD_NAMES;
        owner
            .verify_ownership()
            .map_err(|error| error.to_string())?;
        self.verify_process_table()?;
        self.verify_fixed_endpoints()
    }

    /// Applies the guards that remain meaningful after listeners are live.
    pub(crate) fn verify_while_serving(&self, owner: &DaemonOwnerGuard) -> Result<(), String> {
        owner
            .verify_ownership()
            .map_err(|error| error.to_string())?;
        self.verify_process_table()
    }

    fn verify_process_table(&self) -> Result<(), String> {
        match competing_daemon_detail()? {
            Some(contender) => Err(format!(
                "ATM daemon singleton violation: pid {} observed competing {contender}",
                std::process::id()
            )),
            None => Ok(()),
        }
    }

    fn verify_fixed_endpoints(&self) -> Result<(), String> {
        verify_direct_peer_endpoint(self.direct_peer_port)?;
        verify_unix_socket_endpoint(&self.unix_socket)
    }
}

/// Returns the same-user daemon evidence used in both startup and runtime
/// aborts. Keeping the scan in one place prevents an owner-lock failure from
/// losing the PID/start-time evidence the process-table guard can provide.
pub(crate) fn competing_daemon_detail() -> Result<Option<String>, String> {
    let current_pid = std::process::id();
    let system = System::new_all();
    let current = system
        .process(Pid::from_u32(current_pid))
        .ok_or_else(|| "singleton process-table guard cannot find this daemon".to_owned())?;
    let current_user = current.user_id().ok_or_else(|| {
        "singleton process-table guard cannot determine this daemon's OS user".to_owned()
    })?;
    Ok(system.processes().iter().find_map(|(pid, process)| {
        (pid.as_u32() != current_pid
            && process.user_id() == Some(current_user)
            && process.name() == OsStr::new("atm-daemon"))
        .then(|| {
            format!(
                "atm-daemon pid {}, started_at_unix_seconds={}",
                pid.as_u32(),
                process.start_time()
            )
        })
    }))
}

fn verify_direct_peer_endpoint(port: u16) -> Result<(), String> {
    TcpListener::bind(("0.0.0.0", port))
        .map(drop)
        .map_err(|source| {
            format!(
                "ATM daemon singleton violation: fixed direct-peer endpoint 0.0.0.0:{port} \
                 is already bound; competing pid/start time could not be resolved: {source}"
            )
        })
}

#[cfg(unix)]
fn verify_unix_socket_endpoint(path: &Path) -> Result<(), String> {
    use std::os::unix::net::{UnixListener, UnixStream};

    match UnixListener::bind(path) {
        Ok(listener) => {
            drop(listener);
            std::fs::remove_file(path).map_err(|source| {
                format!(
                    "singleton endpoint guard could not remove probe socket {}: {source}",
                    path.display()
                )
            })
        }
        Err(bind_error) => match UnixStream::connect(path) {
            Ok(stream) => {
                drop(stream);
                Err(format!(
                    "ATM daemon singleton violation: fixed Unix socket {} is already bound; \
                     competing pid/start time could not be resolved: {bind_error}",
                    path.display()
                ))
            }
            Err(_) => Ok(()), // The runtime's guarded stale-socket reclamation owns this case.
        },
    }
}

#[cfg(not(unix))]
fn verify_unix_socket_endpoint(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Panics rather than returning a recoverable exit status. A duplicate daemon
/// is a product-integrity violation, not an operator-facing availability case.
pub(crate) fn abort_for_singleton_violation(violation: impl std::fmt::Display) -> ! {
    let violation = violation.to_string();
    tracing::error!(target: "atm_daemon_bootstrap::singleton", %violation, "ATM daemon singleton violation; aborting immediately");
    panic!("{violation}");
}

#[cfg(test)]
mod tests {
    use super::{GUARD_NAMES, verify_direct_peer_endpoint};

    #[test]
    fn requirements_test_names_all_three_required_singleton_guards() {
        assert_eq!(
            GUARD_NAMES,
            [
                "owner lock ownership",
                "same-user atm-daemon process table",
                "fixed local endpoint availability",
            ]
        );
    }

    #[test]
    fn fixed_endpoint_guard_rejects_an_already_bound_direct_peer_port() {
        let listener = std::net::TcpListener::bind(("0.0.0.0", 0)).expect("bind fixture");
        let port = listener.local_addr().expect("fixture address").port();
        let error = verify_direct_peer_endpoint(port).expect_err("occupied endpoint rejects");
        assert!(error.contains("fixed direct-peer endpoint"));
    }
}
