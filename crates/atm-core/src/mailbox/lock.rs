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
    _in_process_guard: Option<InProcessMailboxLockGuard>,
}

impl Drop for MailboxLockGuard {
    fn drop(&mut self) {
        let active_sentinel = match capture_active_lock_sentinel(
            self.file.as_ref().expect("lock guard file missing"),
            &self.lock_path,
            &self.owner_record,
        ) {
            Ok(active) => active,
            Err(error) => {
                warn!(
                    code = %AtmErrorCode::MailboxLockFailed,
                    %error,
                    target_path = %self.target_path.display(),
                    lock_path = %self.lock_path.display(),
                    "failed to evaluate mailbox lock sentinel removal"
                );
                false
            }
        };

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

        if active_sentinel
            && let Err(error) = remove_active_lock_sentinel(&self.lock_path, &self.owner_record)
            && error.kind() != io::ErrorKind::NotFound
        {
            warn!(
                code = %AtmErrorCode::MailboxLockFailed,
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
            AtmError::mailbox_lock(format!(
                "failed to create mailbox lock directory {}: {error}",
                parent.display()
            ))
            .with_recovery(
                "Ensure the mailbox directory exists and is writable before retrying the ATM command.",
            )
            .with_source(error)
        })?;
    }

    loop {
        let _ = evict_stale_lock_sentinel(&lock_path);
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
                    _in_process_guard: Some(in_process_guard),
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
        if path.extension().and_then(|ext| ext.to_str()) != Some("lock") {
            continue;
        }
        if evict_stale_lock_sentinel(&path) {
            removed += 1;
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
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| {
            AtmError::mailbox_lock(format!(
                "failed to open mailbox lock {}: {error}",
                lock_path.display()
            ))
            .with_recovery(
                "Ensure the mailbox lock file path is writable and not blocked by permissions before retrying the ATM command.",
            )
            .with_source(error)
        })
}

fn write_lock_owner_record(
    file: &File,
    lock_path: &Path,
    owner_record: &LockOwnerRecord,
) -> Result<(), AtmError> {
    file.set_len(0).map_err(|error| {
        AtmError::mailbox_lock(format!(
            "failed to reset mailbox lock {} before writing owner record: {error}",
            lock_path.display()
        ))
        .with_recovery(
            "Check mailbox lock-file permissions and filesystem health before retrying the ATM command.",
        )
        .with_source(error)
    })?;
    let mut writer = file;
    writer.write_all(owner_record.encode().as_bytes()).map_err(|error| {
        AtmError::mailbox_lock(format!(
            "failed to write mailbox lock owner record to {}: {error}",
            lock_path.display()
        ))
        .with_recovery(
            "Check mailbox lock-file permissions and filesystem health before retrying the ATM command.",
        )
        .with_source(error)
    })
}

fn capture_active_lock_sentinel(
    _file: &File,
    lock_path: &Path,
    owner_record: &LockOwnerRecord,
) -> io::Result<bool> {
    let raw = match fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(raw.trim() == owner_record.encode())
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

fn evict_stale_lock_sentinel(lock_path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(lock_path) else {
        return false;
    };
    let Some(pid) = parse_lock_owner_pid(&raw) else {
        return false;
    };
    if process_is_alive(pid) {
        return false;
    }

    match remove_lock_sentinel_with_retry(lock_path) {
        Ok(()) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            warn!(
                code = %AtmErrorCode::MailboxLockFailed,
                %error,
                lock_path = %lock_path.display(),
                pid,
                "failed to evict stale mailbox lock sentinel"
            );
            false
        }
    }
}

fn remove_lock_sentinel_with_retry(lock_path: &Path) -> io::Result<()> {
    let mut last_error = None;
    for attempt in 0..REMOVE_RETRY_ATTEMPTS {
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

    Err(last_error.unwrap_or_else(|| io::Error::other("lock sentinel removal failed")))
}

fn should_retry_remove_lock_sentinel(error: &io::Error) -> bool {
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

#[derive(Clone, Debug)]
struct LockOwnerRecord {
    pid: u32,
    token: u64,
}

impl LockOwnerRecord {
    fn new(pid: u32) -> Self {
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
        let mut held = self.state.held.lock().expect("in-process lock poisoned");
        *held = false;
        self.state.wake.notify_one();
    }
}

fn acquire_in_process_lock(
    path: &Path,
    deadline: Instant,
) -> Result<InProcessMailboxLockGuard, AtmError> {
    let key = canonical_lock_key(path);
    let state = in_process_lock_state(key);
    let mut held = state
        .held
        .lock()
        .map_err(|_| AtmError::mailbox_lock("in-process mailbox lock state poisoned"))?;
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
            .map_err(|_| AtmError::mailbox_lock("in-process mailbox lock wait poisoned"))?;
        held = next_held;
        if wait_result.timed_out() && *held {
            return Err(AtmError::mailbox_lock_timeout(path));
        }
    }
}

fn in_process_lock_state(key: String) -> Arc<InProcessMailboxLockState> {
    static REGISTRY: OnceLock<
        Mutex<std::collections::HashMap<String, Arc<InProcessMailboxLockState>>>,
    > = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut registry = registry
        .lock()
        .expect("in-process mailbox lock registry poisoned");
    registry
        .entry(key)
        .or_insert_with(|| {
            Arc::new(InProcessMailboxLockState {
                held: Mutex::new(false),
                wake: Condvar::new(),
            })
        })
        .clone()
}

fn canonical_lock_key(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use tempfile::tempdir;

    use super::{
        DEFAULT_LOCK_TIMEOUT, acquire, acquire_many_sorted, default_lock_timeout,
        evict_stale_lock_sentinel, is_lock_contention_error, sentinel_path,
        sweep_stale_lock_sentinels,
    };
    use crate::error::AtmErrorCode;

    #[test]
    fn sentinel_path_appends_lock_suffix() {
        let path = PathBuf::from("team-lead.json");
        assert_eq!(sentinel_path(&path), PathBuf::from("team-lead.json.lock"));
    }

    #[test]
    fn acquire_creates_sentinel_file() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");

        let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");

        assert!(sentinel_path(&inbox).exists());
    }

    #[test]
    fn dropping_guard_removes_sentinel_file() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let sentinel = sentinel_path(&inbox);

        {
            let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");
            assert!(sentinel.exists());
        }

        assert_path_eventually_absent(&sentinel);
    }

    #[test]
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
    fn evict_stale_lock_sentinel_removes_dead_pid_file() {
        let tempdir = tempdir().expect("tempdir");
        let sentinel = tempdir.path().join("arch-ctm.json.lock");
        std::fs::write(&sentinel, u32::MAX.to_string()).expect("stale sentinel");

        assert!(evict_stale_lock_sentinel(&sentinel));
        assert_path_eventually_absent(&sentinel);
    }

    #[test]
    fn sweep_stale_lock_sentinels_removes_only_lock_files_with_dead_pids() {
        let tempdir = tempdir().expect("tempdir");
        let lock_path = tempdir.path().join("arch-ctm.json.lock");
        let inbox_path = tempdir.path().join("arch-ctm.json");
        std::fs::write(&lock_path, u32::MAX.to_string()).expect("stale sentinel");
        std::fs::write(&inbox_path, "inbox").expect("inbox");

        let removed = sweep_stale_lock_sentinels(tempdir.path()).expect("sweep");

        assert_eq!(removed, 1);
        assert_path_eventually_absent(&lock_path);
        assert!(inbox_path.exists());
    }

    fn assert_path_eventually_absent(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while path.exists() {
            assert!(
                Instant::now() < deadline,
                "path still exists after bounded wait: {}",
                path.display()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
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
    fn acquire_reports_mailbox_lock_timeout_code() {
        let tempdir = tempdir().expect("tempdir");
        let inbox = tempdir.path().join("arch-ctm.json");
        let _first = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("first lock");

        let error = acquire(&inbox, Duration::from_millis(10)).expect_err("timeout");
        assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);
    }

    #[test]
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
    fn acquire_many_sorted_uses_total_timeout_budget() {
        let tempdir = tempdir().expect("tempdir");
        let first = tempdir.path().join("first.json");
        let blocked = tempdir.path().join("blocked.json");
        let _blocked_guard = acquire(&blocked, DEFAULT_LOCK_TIMEOUT).expect("blocked");

        let started = Instant::now();
        let _ = acquire_many_sorted(vec![first, blocked], Duration::from_millis(50))
            .expect_err("timeout");

        assert!(started.elapsed() < Duration::from_millis(250));
    }

    #[test]
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
    fn default_lock_timeout_uses_default_without_override() {
        assert_eq!(default_lock_timeout(), DEFAULT_LOCK_TIMEOUT);
    }

    #[test]
    fn would_block_is_classified_as_lock_contention() {
        let error = io::Error::from(io::ErrorKind::WouldBlock);
        assert!(is_lock_contention_error(&error));
    }

    use std::path::PathBuf;
}
