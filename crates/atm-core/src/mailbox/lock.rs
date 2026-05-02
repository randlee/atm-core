use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use same_file::Handle;
use tracing::warn;

use crate::error::{AtmError, AtmErrorCode};
use crate::process::process_is_alive;

pub(crate) const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
/// Polling interval between advisory lock acquisition retries.
///
/// A short retry interval keeps contention responsive without spinning hard on
/// the lock sentinel file.
const RETRY_INTERVAL: Duration = Duration::from_millis(50);
const REMOVE_RETRY_INTERVAL: Duration = Duration::from_millis(10);
const REMOVE_RETRY_ATTEMPTS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockOperation {
    CreateDirectory,
    Open,
    WriteOwnerRecord,
    Remove,
    RemoveStaleSentinel,
}

impl LockOperation {
    const fn test_override_token(self) -> &'static str {
        match self {
            Self::CreateDirectory => "create_directory",
            Self::Open => "open",
            Self::WriteOwnerRecord => "write_owner",
            Self::Remove => "remove",
            Self::RemoveStaleSentinel => "remove_stale_sentinel",
        }
    }
}

impl std::fmt::Display for LockOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::CreateDirectory => "create",
            Self::Open => "open",
            Self::WriteOwnerRecord => "write owner record",
            Self::Remove => "remove",
            Self::RemoveStaleSentinel => "remove stale sentinel",
        })
    }
}

/// Canonical in-process registry key for one mailbox lock target.
///
/// The key uses the canonical path when available and otherwise falls back to
/// the provided path. This mirrors `sort_unique_paths()` so the in-process
/// registry and multi-lock planner share the same identity semantics.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CanonicalLockKey(String);

pub(crate) fn default_lock_timeout() -> Duration {
    if let Some(timeout) = debug_timeout_override() {
        return timeout;
    }

    DEFAULT_LOCK_TIMEOUT
}

#[derive(Debug)]
pub(crate) struct MailboxLockGuard {
    target_path: PathBuf,
    lock_path: PathBuf,
    file: Option<File>,
    owner_record: LockOwnerRecord,
    _in_process_guard: InProcessMailboxLockGuard,
}

impl Drop for MailboxLockGuard {
    fn drop(&mut self) {
        let Some(file) = self.file.take() else {
            return;
        };

        if let Err(error) = file.unlock() {
            warn!(
                code = %AtmErrorCode::MailboxLockFailed,
                %error,
                target_path = %self.target_path.display(),
                lock_path = %self.lock_path.display(),
                "failed to release mailbox lock"
            );
        }
        drop(file);

        if let Err(error) = remove_active_lock_sentinel(&self.lock_path, &self.owner_record)
            && error.kind() != io::ErrorKind::NotFound
        {
            warn!(
                code = %mailbox_lock_error_code(&error),
                %error,
                target_path = %self.target_path.display(),
                lock_path = %self.lock_path.display(),
                "failed to remove mailbox lock sentinel"
            );
        }
    }
}

/// Return the sentinel lock-file path for a mailbox file.
pub(crate) fn sentinel_path(path: &Path) -> PathBuf {
    let mut os = path.as_os_str().to_os_string();
    os.push(".lock");
    PathBuf::from(os)
}

/// Sort and deduplicate mailbox paths by canonical filesystem identity.
///
/// Canonicalization failures fall back to the original path string for sorting
/// and deduplication.
pub(crate) fn sort_unique_paths(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut paths = paths
        .into_iter()
        .map(|path| {
            let key = fs::canonicalize(&path)
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned();
            (key, path)
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| left.0.cmp(&right.0));
    paths.dedup_by(|left, right| left.0 == right.0);
    paths.into_iter().map(|(_, path)| path).collect()
}

/// Acquire the advisory lock for one mailbox path.
///
/// # Errors
///
/// Returns [`AtmError`] when the lock sentinel cannot be created/opened or when
/// acquisition either fails fast with
/// [`crate::error_codes::AtmErrorCode::MailboxLockFailed`] or times out with
/// [`crate::error_codes::AtmErrorCode::MailboxLockTimeout`] before the
/// configured deadline.
pub(crate) fn acquire(path: &Path, timeout: Duration) -> Result<MailboxLockGuard, AtmError> {
    let lock_path = sentinel_path(path);
    let owner_pid = std::process::id();
    let owner_record = LockOwnerRecord::new(owner_pid);
    let deadline = Instant::now() + timeout;
    let in_process_guard = acquire_in_process_lock(path, deadline)?;
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            mailbox_lock_path_error(LockOperation::CreateDirectory, parent, error)
        })?;
    }

    loop {
        match evict_stale_lock_sentinel(&lock_path) {
            Ok(_) => {}
            Err(error) if is_readonly_filesystem_error(&error) => {
                return Err(mailbox_lock_path_error(
                    LockOperation::RemoveStaleSentinel,
                    &lock_path,
                    error,
                ));
            }
            Err(error) => {
                warn!(
                    code = %mailbox_lock_error_code(&error),
                    %error,
                    lock_path = %lock_path.display(),
                    "failed to evict stale mailbox lock sentinel during acquisition"
                );
            }
        }
        let file = open_lock_file(&lock_path)?;
        match try_lock_exclusive(&file, &lock_path) {
            Ok(()) => {
                if !lock_path_matches_file(&file, &lock_path)? {
                    let _ = file.unlock();
                    drop(file);
                    if Instant::now() >= deadline {
                        return Err(AtmError::mailbox_lock_timeout(path));
                    }
                    thread::sleep(RETRY_INTERVAL);
                    continue;
                }
                write_lock_owner_record(&file, &lock_path, &owner_record)?;
                return Ok(MailboxLockGuard {
                    target_path: path.to_path_buf(),
                    lock_path,
                    file: Some(file),
                    owner_record,
                    _in_process_guard: in_process_guard,
                });
            }
            Err(error) if is_lock_contention_error(&error) && Instant::now() >= deadline => {
                return Err(AtmError::mailbox_lock_timeout(path).with_source(error));
            }
            Err(error) if is_lock_contention_error(&error) => {
                drop(file);
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => {
                return Err(
                    AtmError::mailbox_lock(format!(
                        "failed to acquire mailbox lock {}: {error}",
                        lock_path.display()
                    ))
                    .with_recovery(
                        "Check mailbox lock-file permissions, parent-directory writability, and filesystem health before retrying the ATM command.",
                    )
                    .with_source(error),
                );
            }
        }
    }
}

/// Sweep one mailbox directory for stale `.lock` sentinels.
///
/// # Errors
///
/// Returns [`AtmError`] only when directory enumeration fails or when stale
/// sentinel cleanup hits a read-only filesystem. Other per-sentinel eviction
/// failures are logged and skipped so recovery commands can continue scanning
/// the rest of the mailbox directory.
pub(crate) fn sweep_stale_lock_sentinels(dir: &Path) -> Result<usize, AtmError> {
    if !dir.exists() {
        return Ok(0);
    }

    let entries = fs::read_dir(dir).map_err(|error| {
        AtmError::mailbox_lock(format!(
            "failed to read mailbox directory {} for stale lock cleanup: {error}",
            dir.display()
        ))
        .with_recovery("Check mailbox directory permissions and retry the ATM command.")
        .with_source(error)
    })?;
    let mut removed = 0usize;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !is_lock_sentinel_candidate(&path) {
            continue;
        }
        match evict_stale_lock_sentinel(&path) {
            Ok(StaleLockSentinelEviction::Removed) => removed += 1,
            Ok(StaleLockSentinelEviction::Skipped) => {}
            Err(error) if is_readonly_filesystem_error(&error) => {
                return Err(mailbox_lock_path_error(
                    LockOperation::RemoveStaleSentinel,
                    &path,
                    error,
                ));
            }
            Err(error) => {
                warn!(
                    code = %mailbox_lock_error_code(&error),
                    %error,
                    lock_path = %path.display(),
                    "failed to evict stale mailbox lock sentinel during sweep"
                );
            }
        }
    }

    Ok(removed)
}

/// Acquire one sorted, deduplicated lock set under a single total timeout budget.
///
/// # Errors
///
/// Returns [`AtmError`] when any mailbox lock cannot be acquired before the
/// total timeout expires. Previously acquired guards are released on failure.
pub(crate) fn acquire_many_sorted(
    paths: impl IntoIterator<Item = PathBuf>,
    timeout: Duration,
) -> Result<Vec<MailboxLockGuard>, AtmError> {
    let paths = sort_unique_paths(paths);
    let deadline = Instant::now() + timeout;
    let mut guards = Vec::with_capacity(paths.len());

    for path in paths {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AtmError::mailbox_lock_timeout(&path));
        }
        guards.push(acquire(&path, remaining)?);
    }

    Ok(guards)
}

fn try_lock_exclusive(file: &File, lock_path: &Path) -> io::Result<()> {
    if std::env::var_os("ATM_TEST_FORCE_LOCK_NON_CONTENTION_ERROR").is_some() {
        return Err(io::Error::other(format!(
            "synthetic non-contention lock failure for {}",
            lock_path.display()
        )));
    }

    file.try_lock_exclusive()
}

fn open_lock_file(lock_path: &Path) -> Result<File, AtmError> {
    if let Some(error) = forced_readonly_filesystem_error(LockOperation::Open) {
        return Err(mailbox_lock_path_error(
            LockOperation::Open,
            lock_path,
            error,
        ));
    }

    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| mailbox_lock_path_error(LockOperation::Open, lock_path, error))
}

fn write_lock_owner_record(
    file: &File,
    lock_path: &Path,
    owner_record: &LockOwnerRecord,
) -> Result<(), AtmError> {
    if let Some(error) = forced_readonly_filesystem_error(LockOperation::WriteOwnerRecord) {
        return Err(mailbox_lock_path_error(
            LockOperation::WriteOwnerRecord,
            lock_path,
            error,
        ));
    }

    file.set_len(0).map_err(|error| {
        mailbox_lock_path_error(LockOperation::WriteOwnerRecord, lock_path, error)
    })?;
    let mut writer = file;
    writer
        .write_all(owner_record.encode().as_bytes())
        .map_err(|error| mailbox_lock_path_error(LockOperation::WriteOwnerRecord, lock_path, error))
}

fn remove_active_lock_sentinel(lock_path: &Path, owner_record: &LockOwnerRecord) -> io::Result<()> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if raw.trim() != owner_record.encode() {
        return Ok(());
    }

    remove_lock_sentinel_with_retry(lock_path)
}

fn lock_path_matches_file(file: &File, lock_path: &Path) -> Result<bool, AtmError> {
    lock_path_matches_file_io(file, lock_path).map_err(|error| {
        AtmError::mailbox_lock(format!(
            "failed to compare mailbox lock identity for {}: {error}",
            lock_path.display()
        ))
        .with_recovery(
            "Check mailbox lock-file permissions and filesystem health before retrying the ATM command.",
        )
        .with_source(error)
    })
}

fn lock_path_matches_file_io(file: &File, lock_path: &Path) -> io::Result<bool> {
    let identity = lock_file_identity_from_file(file)?;
    lock_path_matches_identity(&identity, lock_path)
}

type LockFileIdentity = Handle;

fn lock_path_matches_identity(identity: &LockFileIdentity, lock_path: &Path) -> io::Result<bool> {
    let current = match lock_file_identity_from_path(lock_path) {
        Ok(identity) => identity,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(&current == identity)
}

fn lock_file_identity_from_file(file: &File) -> io::Result<LockFileIdentity> {
    Handle::from_file(file.try_clone()?)
}

fn lock_file_identity_from_path(path: &Path) -> io::Result<LockFileIdentity> {
    Handle::from_path(path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StaleLockSentinelEviction {
    Removed,
    Skipped,
}

fn evict_stale_lock_sentinel(lock_path: &Path) -> io::Result<StaleLockSentinelEviction> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(StaleLockSentinelEviction::Skipped);
        }
        Err(_) => return Ok(StaleLockSentinelEviction::Skipped),
    };
    let Some(pid) = parse_lock_owner_pid(&raw) else {
        return Ok(StaleLockSentinelEviction::Skipped);
    };
    if process_is_alive(pid) {
        return Ok(StaleLockSentinelEviction::Skipped);
    }

    match remove_lock_sentinel_with_retry(lock_path) {
        Ok(()) => Ok(StaleLockSentinelEviction::Removed),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(StaleLockSentinelEviction::Removed)
        }
        Err(error) => Err(error),
    }
}

fn remove_lock_sentinel_with_retry(lock_path: &Path) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..REMOVE_RETRY_ATTEMPTS {
        if let Some(error) = forced_readonly_filesystem_error(LockOperation::Remove) {
            return Err(error);
        }
        match fs::remove_file(lock_path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if should_retry_remove_lock_sentinel(&error) => {
                last_error = Some(error);
                if attempt + 1 < REMOVE_RETRY_ATTEMPTS {
                    thread::sleep(REMOVE_RETRY_INTERVAL);
                }
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.expect("last_error must be Some after retry exhaustion"))
}

fn should_retry_remove_lock_sentinel(error: &io::Error) -> bool {
    if is_readonly_filesystem_error(error) {
        return false;
    }

    if error.kind() == io::ErrorKind::PermissionDenied {
        return true;
    }

    #[cfg(windows)]
    {
        return matches!(
            error.raw_os_error(),
            Some(code)
                if code == windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32
                    || code == windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED as i32
        );
    }

    #[cfg(not(windows))]
    {
        false
    }
}

fn is_lock_sentinel_candidate(path: &Path) -> bool {
    // Sweep both the live `.lock` sentinel and rotated leftovers such as
    // `.lock.old` so crash/recovery cleanup does not miss renamed stale files.
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".lock") || name.contains(".lock."))
}

fn mailbox_lock_error_code(error: &io::Error) -> AtmErrorCode {
    if is_readonly_filesystem_error(error) {
        AtmErrorCode::MailboxLockReadOnlyFilesystem
    } else {
        AtmErrorCode::MailboxLockFailed
    }
}

fn mailbox_lock_path_error(
    operation: LockOperation,
    lock_path: &Path,
    error: io::Error,
) -> AtmError {
    if is_readonly_filesystem_error(&error) {
        AtmError::mailbox_lock_read_only_filesystem(operation, lock_path).with_source(error)
    } else {
        AtmError::mailbox_lock(format!(
            "failed to {operation} mailbox lock {}: {error}",
            lock_path.display()
        ))
        .with_recovery(
            "Check mailbox lock-file permissions, parent-directory writability, and filesystem health before retrying the ATM command.",
        )
        .with_source(error)
    }
}

fn is_readonly_filesystem_error(error: &io::Error) -> bool {
    // Keep this predicate in sync with `readonly_filesystem_raw_os_error()`.
    // Tests inject the raw OS code through that helper and expect this logic to
    // classify the injected error as read-only on every supported platform.
    #[cfg(windows)]
    {
        matches!(
            error.raw_os_error(),
            Some(code) if code == windows_sys::Win32::Foundation::ERROR_WRITE_PROTECT as i32
        )
    }

    #[cfg(not(windows))]
    {
        matches!(error.raw_os_error(), Some(code) if code == libc::EROFS)
    }
}

fn forced_readonly_filesystem_error(operation: LockOperation) -> Option<io::Error> {
    #[cfg(test)]
    if forced_readonly_filesystem_test_override() == Some(operation) {
        return Some(io::Error::from_raw_os_error(
            readonly_filesystem_raw_os_error(),
        ));
    }

    let forced = std::env::var("ATM_TEST_FORCE_LOCK_READONLY_FS").ok()?;
    if forced != operation.test_override_token() {
        return None;
    }

    Some(io::Error::from_raw_os_error(
        readonly_filesystem_raw_os_error(),
    ))
}

#[cfg(windows)]
const fn readonly_filesystem_raw_os_error() -> i32 {
    windows_sys::Win32::Foundation::ERROR_WRITE_PROTECT as i32
}

#[cfg(not(windows))]
const fn readonly_filesystem_raw_os_error() -> i32 {
    libc::EROFS
}

fn is_lock_contention_error(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::WouldBlock {
        return true;
    }

    #[cfg(unix)]
    {
        matches!(
            error.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        )
    }

    #[cfg(windows)]
    {
        return matches!(
            error.raw_os_error(),
            Some(code)
                if code == windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION as i32
                    || code == windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION as i32
        );
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

fn debug_timeout_override() -> Option<Duration> {
    std::env::var("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
}

/// Per-acquisition sentinel ownership record written into the `.lock` file.
///
/// The `(pid, token)` pair lets ATM distinguish the active guard from stale or
/// replaced lock files when deciding whether this process should remove a
/// sentinel during cleanup.
#[derive(Clone, Debug)]
struct LockOwnerRecord {
    pid: u32,
    token: u64,
}

impl LockOwnerRecord {
    fn new(pid: u32) -> Self {
        // Relaxed is sufficient because the token only needs process-local
        // uniqueness for sentinel ownership records; it does not synchronize any
        // shared memory access across threads.
        static NEXT_LOCK_TOKEN: AtomicU64 = AtomicU64::new(1);
        Self {
            pid,
            token: NEXT_LOCK_TOKEN.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn encode(&self) -> String {
        format!("{}:{}", self.pid, self.token)
    }
}

fn parse_lock_owner_pid(raw: &str) -> Option<u32> {
    raw.trim()
        .split_once(':')
        .map_or_else(|| raw.trim().parse().ok(), |(pid, _)| pid.parse().ok())
}

/// Process-local lock state used to serialize repeated lock attempts on the
/// same mailbox path before the advisory file lock is reached.
///
/// The `held` boolean tracks only whether this process currently owns the local
/// gate. The outer registry keeps one shared state per canonical mailbox path.
#[derive(Debug)]
struct InProcessMailboxLockState {
    held: Mutex<bool>,
    wake: Condvar,
}

#[derive(Debug)]
struct InProcessMailboxLockGuard {
    state: Arc<InProcessMailboxLockState>,
}

impl Drop for InProcessMailboxLockGuard {
    fn drop(&mut self) {
        // If the mutex is poisoned during teardown, prefer releasing the local
        // gate over panicking in Drop and aborting the process.
        let mut held = self
            .state
            .held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *held = false;
        self.state.wake.notify_one();
    }
}

fn acquire_in_process_lock(
    path: &Path,
    deadline: Instant,
) -> Result<InProcessMailboxLockGuard, AtmError> {
    let key = canonical_lock_key(path);
    let state = in_process_lock_state(key)?;
    let mut held = state
        .held
        .lock()
        .map_err(|_| {
            AtmError::mailbox_lock("in-process mailbox lock state poisoned").with_recovery(
                "Retry the ATM command. If the error persists, restart the current ATM process so the in-process mailbox lock gate is rebuilt.",
            )
        })?;
    loop {
        if !*held {
            *held = true;
            drop(held);
            return Ok(InProcessMailboxLockGuard { state });
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(AtmError::mailbox_lock_timeout(path));
        }
        let (next_held, wait_result) = state
            .wake
            .wait_timeout(held, remaining)
            .map_err(|_| {
                AtmError::mailbox_lock("in-process mailbox lock wait poisoned").with_recovery(
                    "Retry the ATM command. If the error persists, restart the current ATM process so the in-process mailbox lock gate is rebuilt.",
                )
            })?;
        held = next_held;
        if wait_result.timed_out() && *held {
            return Err(AtmError::mailbox_lock_timeout(path));
        }
    }
}

fn in_process_lock_state(
    key: CanonicalLockKey,
) -> Result<Arc<InProcessMailboxLockState>, AtmError> {
    static REGISTRY: OnceLock<
        Mutex<std::collections::HashMap<CanonicalLockKey, Arc<InProcessMailboxLockState>>>,
    > = OnceLock::new();
    // A coarse process-wide registry mutex is acceptable here because mailbox
    // lock acquisition already goes through retry sleeps and filesystem I/O; the
    // registry only guards short-lived HashMap access before threads block on
    // the per-path condvar.
    let registry = REGISTRY.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut registry = registry
        .lock()
        .map_err(|_| {
            AtmError::mailbox_lock("in-process mailbox lock registry poisoned").with_recovery(
                "Retry the ATM command. If the error persists, restart the current ATM process so the in-process mailbox lock registry is rebuilt.",
            )
        })?;
    Ok(registry
        .entry(key)
        .or_insert_with(|| {
            Arc::new(InProcessMailboxLockState {
                held: Mutex::new(false),
                wake: Condvar::new(),
            })
        })
        .clone())
}

fn canonical_lock_key(path: &Path) -> CanonicalLockKey {
    CanonicalLockKey(
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod readonly_test_override {
    use std::cell::Cell;

    use super::LockOperation;

    thread_local! {
        // Test-only seam for forcing one filesystem operation to fail without
        // introducing shared mutable state across concurrent test threads.
        static OVERRIDE: Cell<Option<LockOperation>> = const { Cell::new(None) };
    }

    pub(super) fn get() -> Option<LockOperation> {
        OVERRIDE.with(|operation| operation.get())
    }

    pub(super) fn set(operation: Option<LockOperation>) -> Option<LockOperation> {
        OVERRIDE.with(|cell| {
            let original = cell.get();
            cell.set(operation);
            original
        })
    }
}

#[cfg(test)]
fn forced_readonly_filesystem_test_override() -> Option<LockOperation> {
    readonly_test_override::get()
}

#[cfg(test)]
mod tests {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::time::Duration;

    use serial_test::serial;
    use tempfile::tempdir;

    use super::{
        DEFAULT_LOCK_TIMEOUT, LockOperation, StaleLockSentinelEviction, acquire,
        acquire_many_sorted, default_lock_timeout, evict_stale_lock_sentinel,
        is_lock_contention_error, is_lock_sentinel_candidate, readonly_test_override,
        sentinel_path, sweep_stale_lock_sentinels,
    };
    use crate::error::AtmErrorCode;

    struct ReadOnlyFilesystemGuard {
        original: Option<LockOperation>,
    }

    impl ReadOnlyFilesystemGuard {
        fn set(operation: LockOperation) -> Self {
            let original = readonly_test_override::set(Some(operation));
            Self { original }
        }
    }

    impl Drop for ReadOnlyFilesystemGuard {
        fn drop(&mut self) {
            readonly_test_override::set(self.original);
        }
    }

    #[test]
    #[serial(env)]
    fn sentinel_path_appends_lock_suffix() {
        let path = PathBuf::from("team-lead.json");
        assert_eq!(sentinel_path(&path), PathBuf::from("team-lead.json.lock"));
    }

    #[test]
    #[serial(env)]
    fn acquire_creates_sentinel_file() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");

        assert!(sentinel_path(&inbox).exists());
    }

    #[test]
    #[serial(env)]
    fn dropping_guard_removes_sentinel_file() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let sentinel = sentinel_path(&inbox);

        {
            let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");
            assert!(sentinel.exists());
        }

        assert!(!sentinel.exists());
    }

    #[test]
    #[serial(env)]
    fn dropping_guard_skips_removal_when_sentinel_path_rotates() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let sentinel = sentinel_path(&inbox);
        let rotated = tempdir.path().join("arch-ctm.json.lock.replaced");

        {
            let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");
            std::fs::rename(&sentinel, &rotated).expect("rotate sentinel");
            std::fs::write(&sentinel, "replacement").expect("replacement sentinel");
        }

        assert!(sentinel.exists());
        assert!(rotated.exists());
    }

    #[test]
    #[serial(env)]
    fn evict_stale_lock_sentinel_removes_dead_pid_file() {
        let tempdir = tempdir().expect("tempdir");
        let sentinel = tempdir.path().join("arch-ctm.json.lock");
        std::fs::write(&sentinel, u32::MAX.to_string()).expect("stale sentinel");

        assert_eq!(
            evict_stale_lock_sentinel(&sentinel).expect("evict"),
            StaleLockSentinelEviction::Removed
        );
        assert!(!sentinel.exists());
    }

    #[test]
    #[serial(env)]
    fn sweep_stale_lock_sentinels_removes_only_lock_files_with_dead_pids() {
        let tempdir = tempdir().expect("tempdir");
        let lock_path = tempdir.path().join("arch-ctm.json.lock");
        let inbox_path = tempdir.path().join("arch-ctm.json");
        std::fs::write(&lock_path, u32::MAX.to_string()).expect("stale sentinel");
        std::fs::write(&inbox_path, "inbox").expect("inbox");

        let removed = sweep_stale_lock_sentinels(tempdir.path()).expect("sweep");

        assert_eq!(removed, 1);
        assert!(!lock_path.exists());
        assert!(inbox_path.exists());
    }

    #[test]
    #[serial(env)]
    fn sweep_stale_lock_sentinels_removes_rotated_dead_pid_sentinels_only() {
        let tempdir = tempdir().expect("tempdir");
        let rotated = tempdir.path().join("arch-ctm.json.lock.old");
        let live_rotated = tempdir.path().join("team-lead.json.lock.replaced");
        let unrelated = tempdir.path().join("locksmith.txt");
        std::fs::write(&rotated, u32::MAX.to_string()).expect("stale rotated");
        std::fs::write(&live_rotated, std::process::id().to_string()).expect("live rotated");
        std::fs::write(&unrelated, u32::MAX.to_string()).expect("unrelated");

        let removed = sweep_stale_lock_sentinels(tempdir.path()).expect("sweep");

        assert_eq!(removed, 1);
        assert!(!rotated.exists());
        assert!(live_rotated.exists());
        assert!(unrelated.exists());
    }

    #[test]
    #[serial(env)]
    fn sweep_stale_lock_sentinels_skips_malformed_rotated_sentinels() {
        let tempdir = tempdir().expect("tempdir");
        let rotated = tempdir.path().join("arch-ctm.json.lock.old");
        std::fs::write(&rotated, "not-a-pid").expect("malformed");

        let removed = sweep_stale_lock_sentinels(tempdir.path()).expect("sweep");

        assert_eq!(removed, 0);
        assert!(rotated.exists());
    }

    #[test]
    #[serial(env)]
    fn is_lock_sentinel_candidate_rejects_partial_lock_suffixes() {
        assert!(!is_lock_sentinel_candidate(&PathBuf::from(
            "inbox.json.lockold",
        )));
        assert!(!is_lock_sentinel_candidate(&PathBuf::from(
            "inbox.locksmith.json",
        )));
    }

    #[test]
    #[serial(env)]
    fn acquire_many_sorted_dedupes_and_sorts_paths() {
        let tempdir = tempdir().expect("tempdir");
        let a = tempdir.path().join("dir").join("..").join("b.json");
        let b = tempdir.path().join("a.json");
        std::fs::create_dir_all(tempdir.path().join("dir")).expect("dir");
        std::fs::write(tempdir.path().join("b.json"), "").expect("b");
        std::fs::write(&b, "").expect("a");

        let guards =
            acquire_many_sorted(vec![a.clone(), b.clone(), a.clone()], DEFAULT_LOCK_TIMEOUT)
                .expect("locks");

        assert_eq!(guards.len(), 2);
    }

    #[test]
    #[serial(env)]
    fn acquire_reports_mailbox_lock_timeout_code() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let _first = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("first lock");

        let error = acquire(&inbox, Duration::from_millis(10)).expect_err("timeout");
        assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);
    }

    #[test]
    #[serial(env)]
    fn acquire_many_sorted_releases_prior_guards_on_failure() {
        let tempdir = tempdir().expect("tempdir");
        let free = tempdir.path().join("free.json");
        let blocked = tempdir.path().join("blocked.json");
        let _blocked_guard = acquire(&blocked, DEFAULT_LOCK_TIMEOUT).expect("blocked");

        let error = acquire_many_sorted(
            vec![free.clone(), blocked.clone()],
            Duration::from_millis(10),
        )
        .expect_err("lock failure");
        assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);

        let _free_guard = acquire(&free, DEFAULT_LOCK_TIMEOUT).expect("free lock released");
    }

    #[test]
    #[serial(env)]
    fn acquire_many_sorted_uses_total_timeout_budget() {
        let tempdir = tempdir().expect("tempdir");
        let first = tempdir.path().join("first.json");
        let blocked = tempdir.path().join("blocked.json");
        let _blocked_guard = acquire(&blocked, DEFAULT_LOCK_TIMEOUT).expect("blocked");

        let error = acquire_many_sorted(vec![first, blocked], Duration::from_millis(50))
            .expect_err("timeout");
        assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);
    }

    #[test]
    #[serial(env)]
    fn sort_unique_paths_dedupes_same_canonical_path() {
        let tempdir = tempdir().expect("tempdir");
        let real = tempdir.path().join("arch-ctm.json");
        std::fs::write(&real, "").expect("write");
        let alternate = tempdir
            .path()
            .join("nested")
            .join("..")
            .join("arch-ctm.json");
        std::fs::create_dir_all(tempdir.path().join("nested")).expect("nested");

        let sorted = super::sort_unique_paths(vec![real.clone(), alternate]);

        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0], real);
    }

    #[test]
    #[serial(env)]
    fn acquire_many_sorted_orders_paths_deterministically() {
        let tempdir = tempdir().expect("tempdir");
        let a = tempdir.path().join("c.json");
        let b = tempdir.path().join("a.json");
        let c = tempdir.path().join("b.json");
        for path in [&a, &b, &c] {
            std::fs::write(path, "").expect("file");
        }

        let sorted = super::sort_unique_paths(vec![a, b.clone(), c]);
        assert_eq!(sorted[0], b);
    }

    #[test]
    #[serial(env)]
    fn default_lock_timeout_uses_default_without_override() {
        assert_eq!(default_lock_timeout(), DEFAULT_LOCK_TIMEOUT);
    }

    #[test]
    #[serial(env)]
    fn would_block_is_classified_as_lock_contention() {
        let error = io::Error::from(io::ErrorKind::WouldBlock);
        assert!(is_lock_contention_error(&error));
    }

    #[test]
    #[serial(env)]
    fn acquire_reports_read_only_filesystem_for_open_failure() {
        let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::Open);
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let error = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect_err("read-only open");

        assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
        assert!(error.message.contains("mailbox lock open failed"));
    }

    #[test]
    #[serial(env)]
    fn acquire_reports_read_only_filesystem_for_open_failure_via_env_var_seam() {
        let _guard = EnvGuard::set_raw("ATM_TEST_FORCE_LOCK_READONLY_FS", "open");
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let error = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect_err("read-only open");

        assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
        assert!(error.message.contains("mailbox lock open failed"));
    }

    #[test]
    #[serial(env)]
    fn acquire_reports_read_only_filesystem_for_owner_record_write_failure() {
        let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::WriteOwnerRecord);
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let error = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect_err("read-only write");

        assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
        assert!(
            error
                .message
                .contains("mailbox lock write owner record failed")
        );
    }

    #[test]
    #[serial(env)]
    fn sweep_reports_read_only_filesystem_for_stale_sentinel_removal() {
        let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::Remove);
        let tempdir = tempdir().expect("tempdir");
        let rotated = tempdir.path().join("arch-ctm.json.lock.old");
        std::fs::write(&rotated, u32::MAX.to_string()).expect("stale rotated");

        let error = sweep_stale_lock_sentinels(tempdir.path()).expect_err("read-only remove");

        assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
        assert!(rotated.exists());
    }

    #[test]
    #[serial(env)]
    fn dropping_guard_tolerates_read_only_cleanup_failure() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let sentinel = sentinel_path(&inbox);
        let guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");
        let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::Remove);

        drop(guard);

        assert!(sentinel.exists());
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<OsString>,
    }

    impl EnvGuard {
        fn set_raw(key: &'static str, value: &str) -> Self {
            let original = std::env::var_os(key);
            set_env_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.take() {
                Some(value) => set_env_var(self.key, value),
                None => remove_env_var(self.key),
            }
        }
    }

    fn set_env_var<K: AsRef<OsStr>, V: AsRef<OsStr>>(key: K, value: V) {
        // SAFETY: env-mutating tests in this module use #[serial(env)] before
        // mutating the process environment, so these mutations are serialized
        // within this process.
        unsafe { std::env::set_var(key, value) }
    }

    fn remove_env_var<K: AsRef<OsStr>>(key: K) {
        // SAFETY: env-mutating tests in this module use #[serial(env)] before
        // mutating the process environment, so these mutations are serialized
        // within this process.
        unsafe { std::env::remove_var(key) }
    }

    use std::path::PathBuf;
}
