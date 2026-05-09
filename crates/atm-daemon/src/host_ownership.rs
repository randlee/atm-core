use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atm_core::error::AtmError;
use fs2::FileExt;

const STALE_OWNER_RECOVERY_RETRY_ATTEMPTS: usize = 3;
pub(crate) const HOST_RUNTIME_OWNER_LOCK_FILE: &str = "owner.lock";
const OWNER_RECOVERY_RETRY_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct HostOwnershipAdapter;

#[derive(Debug)]
pub(crate) struct HostOwnershipGuard {
    lock_file: File,
}

impl HostOwnershipAdapter {
    pub(crate) const fn new() -> Self {
        Self
    }

    pub(crate) fn acquire(&self) -> Result<HostOwnershipGuard, AtmError> {
        Self::acquire_at(atm_core::home::host_runtime_lock_path(
            HOST_RUNTIME_OWNER_LOCK_FILE,
        )?)
    }

    pub(crate) fn acquire_at(
        lock_path: std::path::PathBuf,
    ) -> Result<HostOwnershipGuard, AtmError> {
        let mut lock_file = open_lock_file(&lock_path)?;
        match lock_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                let mut recovered = false;
                if let Some(pid) = recorded_owner_pid(&lock_file)?
                    && !atm_core::process::process_is_alive(pid)
                {
                    drop(lock_file);
                    lock_file = recover_stale_owner_lock(&lock_path, pid)?;
                    recovered = true;
                }
                if !recovered {
                    return Err(AtmError::daemon_serving_state_rejected(format!(
                        "a live ATM daemon already owns {}",
                        lock_path.display()
                    ))
                    .with_source(source)
                    .with_recovery(
                        "Wait for the active daemon to exit or clear a stale owner after verifying the recorded pid is no longer live.",
                    ));
                }
            }
            Err(source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to acquire daemon ownership lock at {}",
                    lock_path.display()
                ))
                .with_source(source));
            }
        }
        write_owner_record(&mut lock_file)?;
        Ok(HostOwnershipGuard { lock_file })
    }
}

impl Drop for HostOwnershipGuard {
    fn drop(&mut self) {
        let _ = clear_owner_record(&mut self.lock_file);
        let _ = self.lock_file.unlock();
    }
}

fn open_lock_file(lock_path: &Path) -> Result<File, AtmError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon lock directory at {}",
                parent.display()
            ))
            .with_source(source)
        })?;
    }

    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|source| {
            AtmError::daemon_unavailable(format!(
                "failed to open daemon ownership lock at {}",
                lock_path.display()
            ))
            .with_source(source)
        })
}

fn recover_stale_owner_lock(lock_path: &Path, stale_pid: u32) -> Result<File, AtmError> {
    for _ in 0..STALE_OWNER_RECOVERY_RETRY_ATTEMPTS {
        thread::sleep(OWNER_RECOVERY_RETRY_INTERVAL);
        let retry_file = open_lock_file(lock_path)?;
        match retry_file.try_lock_exclusive() {
            Ok(()) => return Ok(retry_file),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to retry daemon ownership recovery at {}",
                    lock_path.display()
                ))
                .with_source(source));
            }
        }
    }

    Err(AtmError::daemon_stale_owner_recovery_failed(format!(
        "daemon owner record at {} points to non-live pid {} and the ownership lock could not be safely recovered",
        lock_path.display(),
        stale_pid
    )))
}

fn recorded_owner_pid(lock_file: &File) -> Result<Option<u32>, AtmError> {
    let mut clone = lock_file.try_clone().map_err(|source| {
        AtmError::daemon_unavailable("failed to clone daemon ownership record handle")
            .with_source(source)
    })?;
    clone.seek(SeekFrom::Start(0)).map_err(|source| {
        AtmError::daemon_unavailable("failed to seek daemon ownership record").with_source(source)
    })?;
    let mut record = String::new();
    clone.read_to_string(&mut record).map_err(|source| {
        AtmError::daemon_unavailable("failed to read daemon ownership record").with_source(source)
    })?;
    let trimmed = record.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    Ok(trimmed
        .split_once(':')
        .map(|(pid, _)| pid)
        .unwrap_or(trimmed)
        .parse::<u32>()
        .ok())
}

fn write_owner_record(lock_file: &mut File) -> Result<(), AtmError> {
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|source| {
            AtmError::daemon_unavailable("failed to derive daemon ownership token")
                .with_source(source)
        })?
        .as_nanos();
    lock_file.set_len(0).map_err(|source| {
        AtmError::daemon_unavailable("failed to reset daemon ownership metadata")
            .with_source(source)
    })?;
    writeln!(lock_file, "{}:{token:x}", std::process::id()).map_err(|source| {
        AtmError::daemon_unavailable("failed to write daemon ownership metadata")
            .with_source(source)
    })?;
    lock_file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable("failed to sync daemon ownership metadata").with_source(source)
    })?;
    Ok(())
}

fn clear_owner_record(lock_file: &mut File) -> Result<(), AtmError> {
    lock_file.set_len(0).map_err(|source| {
        AtmError::daemon_unavailable("failed to clear daemon ownership metadata")
            .with_source(source)
    })?;
    lock_file.sync_all().map_err(|source| {
        AtmError::daemon_unavailable("failed to sync cleared daemon ownership metadata")
            .with_source(source)
    })?;
    Ok(())
}
