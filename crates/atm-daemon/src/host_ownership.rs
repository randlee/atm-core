use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use atm_core::error::AtmError;
use fs2::FileExt;
use ulid::Ulid;

#[cfg(test)]
use crate::DaemonSubsystem;
use crate::SubsystemObservability;

#[cfg_attr(windows, allow(dead_code))]
const STALE_OWNER_RECOVERY_RETRY_ATTEMPTS: usize = 3;
#[cfg_attr(windows, allow(dead_code))]
pub(crate) const HOST_RUNTIME_OWNER_LOCK_FILE: &str = "owner.lock";
#[cfg_attr(windows, allow(dead_code))]
const OWNER_RECOVERY_RETRY_INTERVAL: Duration = Duration::from_millis(25);

#[cfg(test)]
struct StaleRecoverySignal {
    observed_tx: std::sync::mpsc::SyncSender<()>,
    continue_rx: std::sync::mpsc::Receiver<()>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerToken(String);

impl OwnerToken {
    fn from_record(record: impl Into<String>) -> Self {
        Self(record.into())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for OwnerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
// Tests inject one stale-recovery rendezvous at a time, so the hook state stays behind a
// process-local mutex instead of raw static items that would race under parallel execution.
static STALE_RECOVERY_OBSERVED_SIGNAL: std::sync::Mutex<Option<StaleRecoverySignal>> =
    std::sync::Mutex::new(None);

#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) struct HostOwnershipAdapter {
    observability: SubsystemObservability,
}

#[cfg_attr(windows, allow(dead_code))]
#[derive(Debug)]
pub(crate) struct HostOwnershipGuard {
    lock_file: File,
    lock_path: PathBuf,
    instance_id: Ulid,
}

#[cfg_attr(windows, allow(dead_code))]
impl HostOwnershipAdapter {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self::new_with_observability(SubsystemObservability::disabled(
            DaemonSubsystem::HostOwnership,
        ))
    }

    pub(crate) fn new_with_observability(observability: SubsystemObservability) -> Self {
        Self { observability }
    }

    pub(crate) fn acquire(&self) -> Result<HostOwnershipGuard, AtmError> {
        self.acquire_at_with_observability(atm_core::home::host_runtime_lock_path(
            HOST_RUNTIME_OWNER_LOCK_FILE,
        )?)
    }

    #[cfg(test)]
    pub(crate) fn acquire_at_home_for_test(
        &self,
        home_dir: &Path,
    ) -> Result<HostOwnershipGuard, AtmError> {
        self.acquire_at_with_observability(atm_core::home::host_runtime_lock_path_from_home(
            home_dir,
            HOST_RUNTIME_OWNER_LOCK_FILE,
        ))
    }

    #[cfg(test)]
    pub(crate) fn acquire_at(
        lock_path: std::path::PathBuf,
    ) -> Result<HostOwnershipGuard, AtmError> {
        Self::new().acquire_at_with_observability(lock_path)
    }

    fn acquire_at_with_observability(
        &self,
        lock_path: std::path::PathBuf,
    ) -> Result<HostOwnershipGuard, AtmError> {
        let mut lock_file = open_lock_file(&lock_path)?;
        match lock_file.try_lock_exclusive() {
            Ok(()) => {}
            Err(source) if is_owner_lock_contention_error(&source) => {
                let mut recovered = false;
                // ADR-002 uses launch.lock for single-launch admission and owner.lock for the
                // actual serving owner so only one daemon can transition into serving state.
                if let Some((pid, token)) = recorded_owner_identity(&lock_file, &lock_path)?
                    && !atm_core::process::process_is_alive(pid)
                {
                    #[cfg(test)]
                    notify_stale_recovery_signal_for_test();
                    drop(lock_file);
                    lock_file = recover_stale_owner_lock(&lock_path, pid, &token)?;
                    recovered = true;
                    self.observability.emit_or_warn(
                        "recover_stale_owner",
                        "degraded",
                        "daemon recovered a stale host-runtime owner lock",
                    );
                }
                if !recovered {
                    self.observability.emit_or_warn(
                        "acquire_owner_lock",
                        "rejected",
                        "daemon host-runtime owner lock is already held by a live process",
                    );
                    return Err(AtmError::daemon_serving_state_rejected(format!(
                        "a live ATM daemon already owns {}",
                        lock_path.display()
                    )));
                }
            }
            Err(_source) => {
                self.observability.emit_or_warn(
                    "acquire_owner_lock",
                    "failed",
                    "daemon failed to acquire the host-runtime owner lock",
                );
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to acquire daemon ownership lock at {}",
                    lock_path.display()
                )));
            }
        }
        let instance_id = write_owner_record(&mut lock_file, &lock_path)?;
        self.observability.emit_or_warn(
            "acquire_owner_lock",
            "ok",
            "daemon acquired the host-runtime owner lock",
        );
        Ok(HostOwnershipGuard {
            lock_file,
            lock_path,
            instance_id,
        })
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn is_owner_lock_contention_error(error: &std::io::Error) -> bool {
    if error.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(windows)]
    {
        // Windows file locking reports contention through raw Win32 lock/sharing
        // violations instead of mapping them to WouldBlock consistently.
        matches!(error.raw_os_error(), Some(32 | 33))
    }

    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg_attr(windows, allow(dead_code))]
impl Drop for HostOwnershipGuard {
    fn drop(&mut self) {
        let _ = clear_owner_record(&mut self.lock_file, &self.lock_path);
        let _ = self.lock_file.unlock();
    }
}

#[cfg_attr(windows, allow(dead_code))]
impl HostOwnershipGuard {
    pub(crate) fn instance_id(&self) -> Ulid {
        self.instance_id
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn open_lock_file(lock_path: &Path) -> Result<File, AtmError> {
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to create daemon lock directory at {}",
                parent.display()
            ))
        })?;
    }

    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to open daemon ownership lock at {}",
                lock_path.display()
            ))
        })
}

#[cfg_attr(windows, allow(dead_code))]
fn recover_stale_owner_lock(
    lock_path: &Path,
    stale_pid: u32,
    stale_token: &OwnerToken,
) -> Result<File, AtmError> {
    for attempt in 0..STALE_OWNER_RECOVERY_RETRY_ATTEMPTS {
        // This retry is only used on stale-owner recovery, not the hot path.
        // OS file-lock APIs do not expose a release notification, so bounded
        // polling is required here and is capped at 3 x 25ms = 75ms total.
        tracing::debug!(
            subsystem = "host_ownership",
            action = "recover_stale_owner_wait",
            attempt = attempt + 1,
            attempts = STALE_OWNER_RECOVERY_RETRY_ATTEMPTS,
            retry_interval_ms = OWNER_RECOVERY_RETRY_INTERVAL.as_millis() as u64,
            stale_pid,
            lock_path = %lock_path.display(),
            "waiting before retrying stale host-runtime owner recovery"
        );
        thread::sleep(OWNER_RECOVERY_RETRY_INTERVAL);
        let retry_file = open_lock_file(lock_path)?;
        match retry_file.try_lock_exclusive() {
            Ok(()) => {
                if owner_record_matches(&retry_file, lock_path, stale_pid, stale_token)? {
                    return Ok(retry_file);
                }
                return Err(owner_token_mismatch_error(lock_path, stale_pid));
            }
            Err(source) if is_owner_lock_contention_error(&source) => {
                if !owner_record_matches(&retry_file, lock_path, stale_pid, stale_token)? {
                    return Err(owner_token_mismatch_error(lock_path, stale_pid));
                }
                continue;
            }
            Err(_source) => {
                return Err(AtmError::daemon_unavailable(format!(
                    "failed to retry daemon ownership recovery at {}",
                    lock_path.display()
                )));
            }
        }
    }

    Err(AtmError::daemon_stale_owner_recovery_failed(format!(
        "daemon owner record at {} points to non-live pid {} and the ownership lock could not be safely recovered",
        lock_path.display(),
        stale_pid
    )))
}

fn recorded_owner_identity(
    lock_file: &File,
    lock_path: &Path,
) -> Result<Option<(u32, OwnerToken)>, AtmError> {
    match read_owner_record_from_handle(lock_file) {
        Ok(identity) => Ok(identity),
        #[cfg(windows)]
        Err(error) if is_owner_lock_contention_error(&error) => {
            read_owner_record_from_shadow_path(lock_path)
        }
        #[cfg(not(windows))]
        Err(error) if is_owner_lock_contention_error(&error) => {
            let _ = error;
            Ok(read_owner_record_from_shadow_path(lock_path))
        }
        Err(_source) => Err(AtmError::daemon_unavailable(
            "failed to read daemon ownership record",
        )),
    }
}

fn read_owner_record_from_handle(
    lock_file: &File,
) -> Result<Option<(u32, OwnerToken)>, std::io::Error> {
    let mut clone = lock_file
        .try_clone()
        .map_err(|source| std::io::Error::new(source.kind(), source.to_string()))?;
    clone.seek(SeekFrom::Start(0))?;
    let mut record = String::new();
    clone.read_to_string(&mut record)?;
    Ok(parse_owner_record(&record))
}

fn parse_owner_record(record: &str) -> Option<(u32, OwnerToken)> {
    let trimmed = record.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut fields = trimmed.splitn(3, ':');
    let pid = fields.next().unwrap_or_default();
    let token = fields.next().unwrap_or_default();
    let pid = pid.parse::<u32>().ok()?;
    Some((pid, OwnerToken::from_record(token)))
}

#[cfg(windows)]
fn owner_record_shadow_path(lock_path: &Path) -> PathBuf {
    let mut name = lock_path
        .file_name()
        .map(|value| value.to_os_string())
        .unwrap_or_else(|| "owner.lock".into());
    name.push(".meta");
    lock_path.with_file_name(name)
}

#[cfg(not(windows))]
fn read_owner_record_from_shadow_path(_lock_path: &Path) -> Option<(u32, OwnerToken)> {
    None
}

#[cfg(windows)]
fn read_owner_record_from_shadow_path(
    lock_path: &Path,
) -> Result<Option<(u32, OwnerToken)>, AtmError> {
    let shadow_path = owner_record_shadow_path(lock_path);
    let record = match fs::read_to_string(&shadow_path) {
        Ok(record) => record,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_source) => {
            return Err(AtmError::daemon_unavailable(format!(
                "failed to read daemon ownership shadow record at {}",
                shadow_path.display()
            )));
        }
    };
    Ok(parse_owner_record(&record))
}

fn sync_owner_record_shadow(lock_path: &Path, record: &str) -> Result<(), AtmError> {
    #[cfg(not(windows))]
    {
        let _ = lock_path;
        let _ = record;
        Ok(())
    }

    #[cfg(windows)]
    {
        let shadow_path = owner_record_shadow_path(lock_path);
        let temp_path = shadow_path.with_file_name(format!(
            ".{}.tmp.{}.shadow",
            shadow_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("owner.lock.meta"),
            std::process::id(),
        ));
        {
            let mut file = File::create(&temp_path).map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to create daemon ownership shadow temp record at {}",
                    temp_path.display()
                ))
            })?;
            file.write_all(record.as_bytes()).map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to write daemon ownership shadow temp record at {}",
                    temp_path.display()
                ))
            })?;
            file.sync_all().map_err(|_source| {
                AtmError::daemon_unavailable(format!(
                    "failed to sync daemon ownership shadow temp record at {}",
                    temp_path.display()
                ))
            })?;
        }
        fs::rename(&temp_path, &shadow_path).map_err(|_source| {
            AtmError::daemon_unavailable(format!(
                "failed to replace daemon ownership shadow record at {}",
                shadow_path.display()
            ))
        })
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn write_owner_record(lock_file: &mut File, lock_path: &Path) -> Result<Ulid, AtmError> {
    let token = OwnerToken::from_record(format!(
        "{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_source| {
                AtmError::daemon_unavailable("failed to derive daemon ownership token")
            })?
            .as_nanos()
    ));
    let instance_id = Ulid::new();
    let record = format!("{}:{token}:{instance_id}\n", std::process::id());
    lock_file.set_len(0).map_err(|_source| {
        AtmError::daemon_unavailable("failed to reset daemon ownership metadata")
    })?;
    write!(lock_file, "{record}").map_err(|_source| {
        AtmError::daemon_unavailable("failed to write daemon ownership metadata")
    })?;
    lock_file.sync_all().map_err(|_source| {
        AtmError::daemon_unavailable("failed to sync daemon ownership metadata")
    })?;
    sync_owner_record_shadow(lock_path, &record)?;
    Ok(instance_id)
}

#[cfg_attr(windows, allow(dead_code))]
fn owner_record_matches(
    lock_file: &File,
    lock_path: &Path,
    expected_pid: u32,
    expected_token: &OwnerToken,
) -> Result<bool, AtmError> {
    Ok(match recorded_owner_identity(lock_file, lock_path)? {
        Some((pid, token)) => pid == expected_pid && token == *expected_token,
        None => false,
    })
}

#[cfg_attr(windows, allow(dead_code))]
fn owner_token_mismatch_error(lock_path: &Path, stale_pid: u32) -> AtmError {
    AtmError::daemon_stale_owner_recovery_failed(format!(
        "daemon owner record at {} changed while recovering stale pid {}",
        lock_path.display(),
        stale_pid
    ))
}

#[cfg_attr(windows, allow(dead_code))]
fn clear_owner_record(lock_file: &mut File, lock_path: &Path) -> Result<(), AtmError> {
    lock_file.set_len(0).map_err(|_source| {
        AtmError::daemon_unavailable("failed to clear daemon ownership metadata")
    })?;
    lock_file.sync_all().map_err(|_source| {
        AtmError::daemon_unavailable("failed to sync cleared daemon ownership metadata")
    })?;
    sync_owner_record_shadow(lock_path, "")?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn recorded_owner_identity_for_guard_for_test(
    guard: &HostOwnershipGuard,
) -> Result<Option<(u32, OwnerToken)>, AtmError> {
    recorded_owner_identity(&guard.lock_file, &guard.lock_path)
}

#[cfg(test)]
pub(crate) fn recorded_owner_identity_at_path_for_test(
    lock_path: &Path,
) -> Result<Option<(u32, OwnerToken)>, AtmError> {
    let file = open_lock_file(lock_path)?;
    recorded_owner_identity(&file, lock_path)
}

#[cfg(test)]
pub(crate) fn install_stale_recovery_signal_for_test(
    observed_tx: std::sync::mpsc::SyncSender<()>,
    continue_rx: std::sync::mpsc::Receiver<()>,
) {
    *STALE_RECOVERY_OBSERVED_SIGNAL
        .lock()
        .expect("stale recovery signal lock") = Some(StaleRecoverySignal {
        observed_tx,
        continue_rx,
    });
}

#[cfg(test)]
pub(crate) fn clear_stale_recovery_signal_for_test() {
    *STALE_RECOVERY_OBSERVED_SIGNAL
        .lock()
        .expect("stale recovery signal lock") = None;
}

#[cfg(test)]
fn notify_stale_recovery_signal_for_test() {
    let signal = STALE_RECOVERY_OBSERVED_SIGNAL
        .lock()
        .expect("stale recovery signal lock");
    if let Some(signal) = signal.as_ref() {
        let _ = signal.observed_tx.send(());
        let _ = signal.continue_rx.recv_timeout(Duration::from_secs(5));
    }
}
