use std::fs;
#[cfg(not(unix))]
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use atm_core::error::AtmError;
use fs2::FileExt;
use serde_json::Value;

#[cfg(not(unix))]
use crate::LocalEndpoint;

#[cfg(not(unix))]
pub(crate) fn bind_loopback_listener() -> Result<(TcpListener, LocalEndpoint), AtmError> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| {
        AtmError::daemon_start_failed(format!("failed to bind local loopback listener: {error}"))
            .with_source(error)
    })?;
    let addr = listener.local_addr().map_err(|error| {
        AtmError::daemon_start_failed("failed to inspect loopback address").with_source(error)
    })?;
    Ok((listener, LocalEndpoint::TcpLoopback(addr)))
}

pub(crate) struct SingletonGuard {
    path: PathBuf,
}

impl SingletonGuard {
    pub(crate) fn acquire(path: &Path) -> Result<Self, AtmError> {
        let stale_eviction_lock_path = path.with_extension("json.lock");
        let stale_eviction_lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&stale_eviction_lock_path)
            .map_err(|error| {
                AtmError::daemon_start_failed(format!(
                    "failed to open daemon singleton stale-eviction lock {}: {error}",
                    stale_eviction_lock_path.display()
                ))
                .with_source(error)
            })?;
        stale_eviction_lock.lock_exclusive().map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to acquire daemon singleton stale-eviction lock {}: {error}",
                stale_eviction_lock_path.display()
            ))
            .with_source(error)
        })?;
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
        {
            Ok(mut file) => {
                let payload = serde_json::json!({ "pid": std::process::id() });
                serde_json::to_writer(&mut file, &payload).map_err(|error| {
                    AtmError::daemon_start_failed("failed to serialize singleton state")
                        .with_source(error)
                })?;
                drop(stale_eviction_lock);
                Ok(Self {
                    path: path.to_path_buf(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let raw = fs::read(path).map_err(|read_error| {
                    AtmError::daemon_already_running(format!(
                        "daemon singleton exists at {} and could not be inspected: {read_error}",
                        path.display()
                    ))
                    .with_source(read_error)
                })?;
                let pid = serde_json::from_slice::<Value>(&raw)
                    .ok()
                    .and_then(|value| value.get("pid").and_then(Value::as_u64))
                    .map(|pid| pid as u32);
                if pid.is_some_and(process_is_alive) {
                    drop(stale_eviction_lock);
                    return Err(AtmError::daemon_already_running(format!(
                        "another ATM daemon already owns {}",
                        path.display()
                    )));
                }
                fs::remove_file(path).map_err(|remove_error| {
                    AtmError::daemon_start_failed(format!(
                        "failed to remove stale daemon singleton {}: {remove_error}",
                        path.display()
                    ))
                    .with_source(remove_error)
                })?;
                drop(stale_eviction_lock);
                Self::acquire(path)
            }
            Err(error) => {
                drop(stale_eviction_lock);
                Err(AtmError::daemon_start_failed(format!(
                    "failed to create daemon singleton {}: {error}",
                    path.display()
                ))
                .with_source(error))
            }
        }
    }

    pub(crate) fn release(&self) -> Result<(), AtmError> {
        fs::remove_file(&self.path).map_err(|error| {
            AtmError::daemon_start_failed(format!(
                "failed to release daemon singleton {}: {error}",
                self.path.display()
            ))
            .with_source(error)
        })
    }
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let pid: libc::pid_t = match pid.try_into() {
        Ok(pid) => pid,
        Err(_) => return false,
    };
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(windows)]
fn process_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut exit_code = 0u32;
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    unsafe { CloseHandle(handle) };
    ok != 0 && exit_code == STILL_ACTIVE as u32
}
