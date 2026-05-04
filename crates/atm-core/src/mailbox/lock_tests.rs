use std::ffi::{OsStr, OsString};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use serial_test::serial;
use tempfile::tempdir;

use super::{
    DEFAULT_LOCK_TIMEOUT, LockOperation, StaleLockSentinelEviction, acquire, acquire_many_sorted,
    default_lock_timeout, evict_stale_lock_sentinel, is_lock_contention_error,
    is_lock_sentinel_candidate, readonly_test_override, sentinel_path, sweep_stale_lock_sentinels,
};
use crate::error::AtmErrorCode;

const TEST_MAILBOX: &str = "test-sender.json";
const TEST_LEAD_MAILBOX: &str = "test-lead.json";

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

#[cfg(windows)]
struct TransientLockIdentityGuard {
    original: usize,
}

#[cfg(windows)]
impl TransientLockIdentityGuard {
    fn set(count: usize) -> Self {
        let original = readonly_test_override::set_transient_lock_identity_errors(count);
        Self { original }
    }
}

#[cfg(windows)]
impl Drop for TransientLockIdentityGuard {
    fn drop(&mut self) {
        readonly_test_override::set_transient_lock_identity_errors(self.original);
    }
}

#[test]
#[serial(env)]
fn sentinel_path_appends_lock_suffix() {
    let path = PathBuf::from(TEST_LEAD_MAILBOX);
    assert_eq!(
        sentinel_path(&path),
        PathBuf::from(format!("{TEST_LEAD_MAILBOX}.lock"))
    );
}

#[test]
#[serial(env)]
fn acquire_creates_sentinel_file() {
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);

    let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");

    assert!(sentinel_path(&inbox).exists());
}

#[test]
#[serial(env)]
fn dropping_guard_removes_sentinel_file() {
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);
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
    let inbox = tempdir.path().join(TEST_MAILBOX);
    let sentinel = sentinel_path(&inbox);
    let rotated = tempdir.path().join(format!("{TEST_MAILBOX}.lock.replaced"));

    {
        let _guard = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock");
        std::fs::rename(&sentinel, &rotated).expect("rotate sentinel");
        std::fs::write(&sentinel, "replacement").expect("replacement sentinel");
    }

    assert!(sentinel.exists());
    assert!(rotated.exists());
}

#[test]
#[cfg(windows)]
#[serial(env)]
fn acquire_retries_when_lock_identity_compare_hits_transient_access_denied() {
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);
    let _guard = TransientLockIdentityGuard::set(1);

    let _lock = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect("lock after transient compare");

    assert!(sentinel_path(&inbox).exists());
}

#[test]
#[serial(env)]
fn evict_stale_lock_sentinel_removes_dead_pid_file() {
    let tempdir = tempdir().expect("tempdir");
    let sentinel = tempdir.path().join(format!("{TEST_MAILBOX}.lock"));
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
    let lock_path = tempdir.path().join(format!("{TEST_MAILBOX}.lock"));
    let inbox_path = tempdir.path().join(TEST_MAILBOX);
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
    let rotated = tempdir.path().join(format!("{TEST_MAILBOX}.lock.old"));
    let live_rotated = tempdir
        .path()
        .join(format!("{TEST_LEAD_MAILBOX}.lock.replaced"));
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
    let rotated = tempdir.path().join(format!("{TEST_MAILBOX}.lock.old"));
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

    let guards = acquire_many_sorted(vec![a.clone(), b.clone(), a.clone()], DEFAULT_LOCK_TIMEOUT)
        .expect("locks");

    assert_eq!(guards.len(), 2);
}

#[test]
#[serial(env)]
fn acquire_reports_mailbox_lock_timeout_code() {
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);
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

    let error =
        acquire_many_sorted(vec![first, blocked], Duration::from_millis(50)).expect_err("timeout");
    assert_eq!(error.code, AtmErrorCode::MailboxLockTimeout);
}

#[test]
#[serial(env)]
fn sort_unique_paths_dedupes_same_canonical_path() {
    let tempdir = tempdir().expect("tempdir");
    let real = tempdir.path().join(TEST_MAILBOX);
    std::fs::write(&real, "").expect("write");
    let alternate = tempdir.path().join("nested").join("..").join(TEST_MAILBOX);
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
    let _guard = EnvGuard::clear("ATM_TEST_MAILBOX_LOCK_TIMEOUT_MS");
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
    let inbox = tempdir.path().join(TEST_MAILBOX);

    let error = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect_err("read-only open");

    assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
    assert!(error.message.contains("mailbox lock open failed"));
}

#[test]
#[serial(env)]
fn acquire_reports_read_only_filesystem_for_open_failure_via_env_var_seam() {
    let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::Open);
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);

    let error = acquire(&inbox, DEFAULT_LOCK_TIMEOUT).expect_err("read-only open");

    assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
    assert!(error.message.contains("mailbox lock open failed"));
}

#[test]
#[serial(env)]
fn acquire_reports_read_only_filesystem_for_owner_record_write_failure() {
    let _readonly = ReadOnlyFilesystemGuard::set(LockOperation::WriteOwnerRecord);
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);

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
    let rotated = tempdir.path().join(format!("{TEST_MAILBOX}.lock.old"));
    std::fs::write(&rotated, u32::MAX.to_string()).expect("stale rotated");

    let error = sweep_stale_lock_sentinels(tempdir.path()).expect_err("read-only remove");

    assert_eq!(error.code, AtmErrorCode::MailboxLockReadOnlyFilesystem);
    assert!(rotated.exists());
}

#[test]
#[serial(env)]
fn dropping_guard_tolerates_read_only_cleanup_failure() {
    let tempdir = tempdir().expect("tempdir");
    let inbox = tempdir.path().join(TEST_MAILBOX);
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
    fn clear(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        remove_env_var(key);
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
