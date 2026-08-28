//! `$ATM_TEMP` TTL sweep: config validation, and one pure sweep pass
//! (ADR-055 decision (b)).
//!
//! This module is deliberately `tokio`-free: `atm-core` stays runtime-agnostic.
//! The periodic-task wrapper (interval scheduling, cancel-then-join shutdown)
//! lives in `atm-daemon-bootstrap`, composed against the replacement
//! Tokio/Axum runtime; this module only performs one bounded, synchronous
//! sweep pass over a given root, so it is trivially unit-testable with a
//! temp directory and an injected clock (the `now` parameter).

use std::fmt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

/// Validated sweep policy: an interval between passes and a TTL applied to
/// every entry under the scratch root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepConfig {
    pub interval: Duration,
    pub ttl: Duration,
}

/// Sweep configuration and pass-fatal failure modes.
///
/// Zero interval/TTL is a config error, but — mirroring `resolve_atm_temp`'s
/// "unset is not a failure" rule — it is only reachable once `ATM_TEMP`
/// itself has resolved; `validate_sweep_config` is called only after that
/// succeeds, never during `.atm.toml` parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweeperError {
    /// `sweep_interval_seconds` was configured as zero.
    ZeroInterval,
    /// `sweep_ttl_days` was configured as zero.
    ZeroTtl,
    /// The scratch root itself is missing, is not a directory, or could not
    /// be read. Per-entry failures below the root are skipped-and-logged,
    /// not pass-fatal; only a root-level condition produces this variant.
    RootUnavailable { path: PathBuf, reason: String },
}

impl fmt::Display for SweeperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroInterval => f.write_str("sweep_interval_seconds must not be zero"),
            Self::ZeroTtl => f.write_str("sweep_ttl_days must not be zero"),
            Self::RootUnavailable { path, reason } => {
                write!(f, "sweep root {} is unavailable: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for SweeperError {}

/// Validates the configured sweep interval and TTL. See [`SweeperError`]'s
/// doc comment for when this is reachable.
///
/// # Errors
///
/// Returns [`SweeperError::ZeroInterval`] or [`SweeperError::ZeroTtl`] when
/// either input is zero.
pub fn validate_sweep_config(
    interval_seconds: u64,
    ttl_days: u32,
) -> Result<SweepConfig, SweeperError> {
    if interval_seconds == 0 {
        return Err(SweeperError::ZeroInterval);
    }
    if ttl_days == 0 {
        return Err(SweeperError::ZeroTtl);
    }
    Ok(SweepConfig {
        interval: Duration::from_secs(interval_seconds),
        ttl: Duration::from_secs(u64::from(ttl_days) * 24 * 60 * 60),
    })
}

/// One completed (or partially completed, if cancelled) sweep pass's
/// structured result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Number of directory entries visited (files, directories, and
    /// symlinks alike).
    pub scanned: u64,
    /// Total bytes reclaimed by removing expired files and symlinks.
    pub reclaimed_bytes: u64,
    /// Number of entries left in place because of a per-entry failure
    /// (permission denied, a race with a concurrent writer, an in-progress
    /// marker, and so on). Never includes entries kept because they are
    /// still fresh.
    pub skipped: u64,
}

/// One entry's expiry-relevant filesystem timestamps: content modification
/// time and metadata/inode change time.
///
/// [`is_expired`] requires **both** to independently be at least `ttl` old
/// (QM43-I6). This guards against a producer that lands a file via an
/// atomic rename into the scratch root: `rename` stamps a fresh `ctime` at
/// the destination path even when the file's *content* `mtime` is already
/// old (for example, preserved from a remote source, or simply because the
/// content was written well before the rename that made it visible here).
/// A single-signal, mtime-only check would misjudge such a just-landed file
/// as long-expired. It is also a secondary guard against an in-flight write
/// (whose `mtime` alone is already fresh under the single-signal rule this
/// replaces).
///
/// On a platform with no ctime-equivalent, `changed` is `None` and the
/// mtime-only signal decides on its own — a documented residual window
/// (ADR-055).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EntryAge {
    pub modified: Option<SystemTime>,
    pub changed: Option<SystemTime>,
}

impl EntryAge {
    /// Builds an [`EntryAge`] from real filesystem `Metadata`. Production
    /// code always goes through this constructor (via
    /// [`RealEntryAgeSource`]); tests that need to exercise the dual
    /// mtime/ctime decision deterministically construct an `EntryAge`
    /// directly instead, because a real inode's `ctime` cannot be
    /// backdated by any safe API — the kernel always stamps it "now" on any
    /// metadata-changing syscall, by design, which makes the usual
    /// `SystemTime`-backdate-a-real-file test technique unusable for the
    /// `ctime` side of this check.
    #[cfg(unix)]
    #[must_use]
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            modified: metadata.modified().ok(),
            changed: unix_epoch_time(metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    /// No portable ctime-equivalent exists on this platform; `changed` is
    /// `None` and [`is_expired`] falls back to the mtime-only signal (a
    /// documented residual window, ADR-055).
    #[cfg(not(unix))]
    #[must_use]
    pub fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            modified: metadata.modified().ok(),
            changed: None,
        }
    }
}

#[cfg(unix)]
fn unix_epoch_time(secs: i64, nsecs: i64) -> Option<SystemTime> {
    let secs = u64::try_from(secs).ok()?;
    let nsecs = u32::try_from(nsecs).ok()?;
    Some(SystemTime::UNIX_EPOCH + Duration::new(secs, nsecs))
}

/// Reads one directory entry's [`EntryAge`]. Every real sweep uses
/// [`RealEntryAgeSource`]; a test-only implementation lets a test inject
/// synthetic ages (see [`EntryAge::from_metadata`]'s doc comment for why
/// that is the only way to deterministically test the ctime-guard branch),
/// or observe/react to each entry visited (used to test the cancellation
/// seam and to simulate a slow filesystem).
pub trait EntryAgeSource {
    fn age_of(&self, path: &Path, metadata: &std::fs::Metadata) -> EntryAge;
}

/// The only production [`EntryAgeSource`]: reads real filesystem metadata.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealEntryAgeSource;

impl EntryAgeSource for RealEntryAgeSource {
    fn age_of(&self, _path: &Path, metadata: &std::fs::Metadata) -> EntryAge {
        EntryAge::from_metadata(metadata)
    }
}

/// Removes everything under `root` whose age (see [`EntryAge`]) is at least
/// `ttl` old, using the real filesystem clock and no cancellation seam.
///
/// Never follows a symlink into whatever it targets: a symlink entry is
/// aged and reclaimed using its own (`lstat`-observed) timestamps, exactly
/// like any other entry, and its target is never visited. This makes
/// "never follows symlinks out of the root" true by construction.
///
/// An entry with an `<entry-name>.inprogress` sibling marker file is never
/// reclaimed regardless of age (skipped, not removed): this is the
/// producer-side seam a future writer uses to protect an in-flight landing
/// beyond what the mtime/ctime dual check alone covers (QM43-I6).
///
/// Per-entry failures (permission denied, a concurrent removal, and so on)
/// are skipped and counted in [`SweepReport::skipped`]; they do not abort
/// the pass. Only a root-level condition — the root itself missing, not a
/// directory, or unreadable — is pass-fatal.
///
/// # Errors
///
/// Returns [`SweeperError::RootUnavailable`] when `root` cannot be
/// inspected as a directory.
pub fn sweep_once(
    root: &Path,
    ttl: Duration,
    now: SystemTime,
) -> Result<SweepReport, SweeperError> {
    sweep_once_cancellable(root, ttl, now, &AtomicBool::new(false))
}

/// As [`sweep_once`], but polls `cancelled` once per entry (and once per
/// directory descended into), so a caller running this on a blocking thread
/// can interrupt an unbounded walk within a bounded shutdown deadline
/// instead of waiting for the whole pass to finish (QM43-I7). This is the
/// function `atm-daemon-bootstrap`'s periodic sweeper composes against for
/// real cancellable shutdown.
///
/// # Errors
///
/// Returns [`SweeperError::RootUnavailable`] when `root` cannot be
/// inspected as a directory.
pub fn sweep_once_cancellable(
    root: &Path,
    ttl: Duration,
    now: SystemTime,
    cancelled: &AtomicBool,
) -> Result<SweepReport, SweeperError> {
    sweep_once_with_age_source(root, ttl, now, &RealEntryAgeSource, cancelled)
}

/// As [`sweep_once_cancellable`], but with an injectable [`EntryAgeSource`]
/// instead of always reading real filesystem metadata. Real sweeps always
/// pass [`RealEntryAgeSource`] (that is what [`sweep_once_cancellable`]
/// does); tests inject a fake source to exercise the ctime-guard branch and
/// the cancellation seam deterministically.
///
/// # Errors
///
/// Returns [`SweeperError::RootUnavailable`] when `root` cannot be
/// inspected as a directory.
pub fn sweep_once_with_age_source(
    root: &Path,
    ttl: Duration,
    now: SystemTime,
    age_source: &dyn EntryAgeSource,
    cancelled: &AtomicBool,
) -> Result<SweepReport, SweeperError> {
    let root_metadata = std::fs::metadata(root).map_err(|error| SweeperError::RootUnavailable {
        path: root.to_path_buf(),
        reason: error.to_string(),
    })?;
    if !root_metadata.is_dir() {
        return Err(SweeperError::RootUnavailable {
            path: root.to_path_buf(),
            reason: "not a directory".to_string(),
        });
    }
    let mut report = SweepReport::default();
    sweep_dir(root, ttl, now, age_source, cancelled, &mut report);
    Ok(report)
}

fn sweep_dir(
    dir: &Path,
    ttl: Duration,
    now: SystemTime,
    age_source: &dyn EntryAgeSource,
    cancelled: &AtomicBool,
    report: &mut SweepReport,
) {
    if cancelled.load(Ordering::Relaxed) {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) => {
            report.skipped += 1;
            warn_sweep_io_error("read_dir", dir, &error);
            return;
        }
    };
    for entry in entries {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                report.skipped += 1;
                warn_sweep_io_error("read_dir_entry", dir, &error);
                continue;
            }
        };
        report.scanned += 1;
        let path = entry.path();
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                report.skipped += 1;
                warn_sweep_io_error("symlink_metadata", &path, &error);
                continue;
            }
        };
        let age = age_source.age_of(&path, &metadata);
        let expired = is_expired(&age, ttl, now);

        if is_symlink_or_reparse_point(&metadata) {
            if expired {
                reclaim_file(&path, &metadata, report);
            }
            continue;
        }

        if metadata.is_dir() {
            sweep_dir(&path, ttl, now, age_source, cancelled, report);
            if cancelled.load(Ordering::Relaxed) {
                // A cancelled descent may have left entries in place below
                // this directory; never remove it on the guess that it is
                // now empty.
                return;
            }
            let now_empty = std::fs::read_dir(&path)
                .map(|mut remaining| remaining.next().is_none())
                .unwrap_or(false);
            if now_empty
                && expired
                && let Err(error) = std::fs::remove_dir(&path)
            {
                report.skipped += 1;
                warn_sweep_io_error("remove_dir", &path, &error);
            }
            continue;
        }

        if expired {
            reclaim_file(&path, &metadata, report);
        }
    }
}

/// Returns whether `metadata` is a reparse point that must never be
/// descended into as an ordinary directory.
///
/// On Unix this is exactly [`std::fs::FileType::is_symlink`]. On Windows,
/// `is_symlink()` only recognizes the `IO_REPARSE_TAG_SYMLINK` tag -- a
/// junction (`IO_REPARSE_TAG_MOUNT_POINT`, created by `mklink /J` or a
/// legacy directory-junction tool) reports `is_dir() == true` and
/// `is_symlink() == false`, so relying on `is_symlink()` alone would let
/// the sweeper recurse straight through a junction and out of the sweep
/// root -- exactly the escape this module's "never follows symlinks out of
/// the root" guarantee exists to prevent. Checked instead via the raw
/// `FILE_ATTRIBUTE_REPARSE_POINT` bit
/// (`std::os::windows::fs::MetadataExt::file_attributes`), which is set for
/// every reparse-point kind, so a junction is refused the same way a
/// symlink already is.
#[cfg(windows)]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

fn reclaim_file(path: &Path, metadata: &std::fs::Metadata, report: &mut SweepReport) {
    if has_inprogress_marker(path) {
        report.skipped += 1;
        return;
    }
    let size = metadata.len();
    match std::fs::remove_file(path) {
        Ok(()) => report.reclaimed_bytes += size,
        Err(error) => {
            report.skipped += 1;
            warn_sweep_io_error("remove_file", path, &error);
        }
    }
}

/// Returns whether `path` has a sibling `<name>.inprogress` marker file.
/// When present, the entry is never reclaimed regardless of age: this is
/// the producer-side seam a future writer uses (create the marker before
/// writing, remove it after) to protect an in-flight landing beyond what
/// the mtime/ctime dual check alone covers. Checked with `symlink_metadata`
/// (never followed) for the same never-follow-symlinks discipline as the
/// rest of this module.
fn has_inprogress_marker(path: &Path) -> bool {
    let mut marker = path.as_os_str().to_owned();
    marker.push(".inprogress");
    std::fs::symlink_metadata(PathBuf::from(marker)).is_ok()
}

fn warn_sweep_io_error(action: &'static str, path: &Path, error: &std::io::Error) {
    tracing::warn!(
        subsystem = "atm_temp_sweeper",
        action,
        outcome = "io_error",
        path = %path.display(),
        %error,
        "atm_temp sweep hit an I/O error for one entry; skipping"
    );
}

fn is_expired(age: &EntryAge, ttl: Duration, now: SystemTime) -> bool {
    let mtime_old = age_at_least(age.modified, ttl, now);
    let ctime_old = match age.changed {
        Some(_) => age_at_least(age.changed, ttl, now),
        // No ctime signal available for this entry/platform: mtime alone
        // decides (documented residual window, ADR-055).
        None => true,
    };
    mtime_old && ctime_old
}

fn age_at_least(t: Option<SystemTime>, ttl: Duration, now: SystemTime) -> bool {
    t.and_then(|t| now.duration_since(t).ok())
        .is_some_and(|age| age >= ttl)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::test_support::capture_tracing;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    const TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

    /// `File::set_modified` is a stable, cross-platform std API (no
    /// platform cfg needed); this helper backdates a real file's mtime for
    /// both the `#[cfg(unix)]` and `#[cfg(windows)]` reparse-point tests
    /// below.
    fn age_file(path: &Path, age: Duration, now: SystemTime) {
        let modified = now
            .checked_sub(age)
            .expect("age must not underflow SystemTime");
        let file = std::fs::File::open(path).expect("open for mtime rewrite");
        file.set_modified(modified).expect("set mtime");
    }

    /// Test-only [`EntryAgeSource`] that supplies an explicit [`EntryAge`]
    /// per path (falling back to "no signal" — never expired — for any path
    /// not registered), so a test can assert the mtime/ctime dual-check
    /// decision deterministically without depending on a real file's
    /// ctime, which cannot be backdated by any safe API.
    #[derive(Default)]
    struct FakeEntryAgeSource(HashMap<PathBuf, EntryAge>);

    impl FakeEntryAgeSource {
        fn with(mut self, path: &Path, age: EntryAge) -> Self {
            self.0.insert(path.to_path_buf(), age);
            self
        }

        fn old(now: SystemTime, extra_age: Duration) -> EntryAge {
            let then = now
                .checked_sub(TTL + extra_age)
                .expect("age must not underflow SystemTime");
            EntryAge {
                modified: Some(then),
                changed: Some(then),
            }
        }
    }

    impl EntryAgeSource for FakeEntryAgeSource {
        fn age_of(&self, path: &Path, _metadata: &std::fs::Metadata) -> EntryAge {
            self.0.get(path).copied().unwrap_or_default()
        }
    }

    #[test]
    fn zero_interval_is_rejected() {
        assert_eq!(
            validate_sweep_config(0, 30),
            Err(SweeperError::ZeroInterval)
        );
    }

    #[test]
    fn zero_ttl_is_rejected() {
        assert_eq!(validate_sweep_config(3600, 0), Err(SweeperError::ZeroTtl));
    }

    #[test]
    fn valid_config_converts_days_to_seconds() {
        let config = validate_sweep_config(3600, 30).expect("valid config");
        assert_eq!(config.interval, Duration::from_secs(3600));
        assert_eq!(config.ttl, Duration::from_secs(30 * 24 * 60 * 60));
    }

    #[test]
    fn missing_root_is_pass_fatal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        let error = sweep_once(&missing, TTL, SystemTime::now()).expect_err("missing root fails");
        assert!(
            matches!(error, SweeperError::RootUnavailable { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn expired_file_is_reclaimed_and_fresh_file_is_kept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        let expired = dir.path().join("expired.bin");
        std::fs::write(&expired, b"0123456789").expect("write expired");
        let fresh = dir.path().join("fresh.bin");
        std::fs::write(&fresh, b"01234").expect("write fresh");

        // Both mtime and ctime must independently look old for `expired` to
        // be reclaimed (QM43-I6); real ctime cannot be backdated, so this
        // uses a fake age source rather than `age_file`. `fresh` is left
        // unregistered, which the fake source reports as "no signal" (never
        // expired).
        let age_source = FakeEntryAgeSource::default().with(
            &expired,
            FakeEntryAgeSource::old(now, Duration::from_secs(60)),
        );

        let report =
            sweep_once_with_age_source(dir.path(), TTL, now, &age_source, &AtomicBool::new(false))
                .expect("sweep succeeds");
        assert_eq!(report.scanned, 2);
        assert_eq!(report.reclaimed_bytes, 10);
        assert_eq!(report.skipped, 0);
        assert!(!expired.exists());
        assert!(fresh.exists());
    }

    // Backdating a directory's own mtime through `File::open` + `set_modified`
    // relies on Unix directory-open semantics (opening a directory via
    // `File::open` is unsupported on Windows without extra flags); the two
    // directory-aging tests below are Unix-only for that reason. File-mtime
    // backdating (used by every other test in this module) is portable.
    #[cfg(unix)]
    #[test]
    fn expired_empty_directory_is_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        let send_to = dir.path().join("send-to");
        let nested = send_to.join("01ABC");
        std::fs::create_dir_all(&nested).expect("nested dir");

        let old = FakeEntryAgeSource::old(now, Duration::from_secs(1));
        let age_source = FakeEntryAgeSource::default()
            .with(&send_to, old)
            .with(&nested, old);

        let report =
            sweep_once_with_age_source(dir.path(), TTL, now, &age_source, &AtomicBool::new(false))
                .expect("sweep succeeds");
        assert!(report.scanned >= 2);
        assert!(!nested.exists());
        assert!(!send_to.exists());
    }

    #[cfg(unix)]
    #[test]
    fn non_empty_directory_is_kept_even_when_aged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        let nested = dir.path().join("send-to").join("01ABC");
        std::fs::create_dir_all(&nested).expect("nested dir");
        let fresh_file = nested.join("report.pdf");
        std::fs::write(&fresh_file, b"pdf").expect("write");
        age_file(&fresh_file, Duration::from_secs(60), now);
        age_file(&nested, TTL + Duration::from_secs(1), now);
        age_file(
            dir.path().join("send-to").as_path(),
            TTL + Duration::from_secs(1),
            now,
        );

        let report = sweep_once(dir.path(), TTL, now).expect("sweep succeeds");
        assert_eq!(report.reclaimed_bytes, 0);
        assert!(fresh_file.exists());
        assert!(nested.exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_is_never_followed() {
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let now = SystemTime::now();

        // The symlink target is deliberately old (well past TTL): if the
        // sweeper ever followed the symlink and evaluated the *target's*
        // age instead of the link's own age, this file would be reclaimed.
        let outside_target = outside.path().join("secret.bin");
        std::fs::write(&outside_target, b"do-not-touch").expect("write outside file");
        age_file(&outside_target, TTL + Duration::from_secs(60), now);

        let link = root.path().join("escape-link");
        std::os::unix::fs::symlink(&outside_target, &link).expect("symlink");
        // The link itself is fresh (just created), so a correct
        // never-follow sweeper must neither remove it nor touch its target.

        let report = sweep_once(root.path(), TTL, now).expect("sweep succeeds");
        assert_eq!(report.scanned, 1);
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "fresh symlink must be kept, not followed (checked without following it)"
        );
        assert!(
            outside_target.exists(),
            "the escape target must never be visited or reclaimed"
        );
    }

    /// Windows twin of `symlink_escape_is_never_followed`: a junction
    /// (`mklink /J`, `IO_REPARSE_TAG_MOUNT_POINT`) pointing outside the
    /// sweep root must never be recursed into or followed, exactly like a
    /// Unix symlink. Uses a junction rather than
    /// `std::os::windows::fs::symlink_dir` because junctions need no
    /// `SeCreateSymbolicLinkPrivilege` (elevated/Developer Mode) on the CI
    /// runner, and because a junction is exactly the case
    /// `is_symlink_or_reparse_point` was added to catch (`is_symlink()`
    /// alone does not recognize `IO_REPARSE_TAG_MOUNT_POINT`).
    #[cfg(windows)]
    #[test]
    fn junction_escape_is_never_followed() {
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let now = SystemTime::now();

        // The junction target is deliberately old (well past TTL): if the
        // sweeper ever followed the junction and evaluated the *target's*
        // age instead of the junction entry's own age, this file would be
        // reclaimed.
        let outside_dir = outside.path().join("secret-dir");
        std::fs::create_dir(&outside_dir).expect("create outside dir");
        let outside_marker = outside_dir.join("secret.bin");
        std::fs::write(&outside_marker, b"do-not-touch").expect("write outside marker");
        age_file(&outside_marker, TTL + Duration::from_secs(60), now);

        let link = root.path().join("escape-junction");
        create_junction(&outside_dir, &link);
        // The junction entry itself is fresh (just created), so a correct
        // never-follow sweeper must neither remove it nor touch its target.

        let report = sweep_once(root.path(), TTL, now).expect("sweep succeeds");
        assert_eq!(report.scanned, 1);
        assert!(
            std::fs::symlink_metadata(&link).is_ok(),
            "fresh junction must be kept, not followed (checked without following it)"
        );
        assert!(
            outside_marker.exists(),
            "the escape target must never be visited or reclaimed"
        );
    }

    /// Creates a real NTFS junction at `link` pointing at `target` via the
    /// `mklink /J` shell built-in (no admin/Developer Mode privilege
    /// required, unlike a true directory symlink).
    #[cfg(windows)]
    fn create_junction(target: &Path, link: &Path) {
        let status = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &link.display().to_string(),
                &target.display().to_string(),
            ])
            .status()
            .expect("spawn mklink /J");
        assert!(
            status.success(),
            "mklink /J must succeed to create the test junction"
        );
    }

    #[cfg(unix)]
    #[test]
    fn skipped_counts_a_read_failure_without_aborting_the_pass() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();

        let ok_file = dir.path().join("ok.bin");
        std::fs::write(&ok_file, b"x").expect("write");

        let locked_dir = dir.path().join("locked");
        std::fs::create_dir(&locked_dir).expect("create locked dir");
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o000))
            .expect("chmod locked dir unreadable");

        let age_source = FakeEntryAgeSource::default()
            .with(
                &ok_file,
                FakeEntryAgeSource::old(now, Duration::from_secs(1)),
            )
            .with(
                &locked_dir,
                FakeEntryAgeSource::old(now, Duration::from_secs(1)),
            );

        let (result, logs) = capture_tracing(|| {
            sweep_once_with_age_source(dir.path(), TTL, now, &age_source, &AtomicBool::new(false))
        });
        let report = result.expect("sweep succeeds despite one failure");

        // Restore permissions so `tempfile` can clean up the directory,
        // regardless of the assertions below.
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore locked dir permissions");

        // The discarded `io::Error` behind the skip must not vanish
        // silently: it is logged at `warn` with structured
        // `subsystem`/`action`/`outcome` fields (QM43-I2/QM43-I6 cluster).
        assert!(
            logs.contains("subsystem=\"atm_temp_sweeper\"")
                && logs.contains("action=\"read_dir\"")
                && logs.contains("outcome=\"io_error\""),
            "expected a structured warn for the discarded io::Error, got: {logs}"
        );

        assert_eq!(report.skipped, 1);
        assert!(
            !ok_file.exists(),
            "the unrelated expired file is still reclaimed"
        );
    }

    /// A file whose mtime looks old but whose ctime is fresh (as if it had
    /// just landed via an atomic rename that preserved an old content
    /// mtime) must not be reclaimed (QM43-I6).
    #[test]
    fn mtime_old_but_ctime_fresh_is_not_expired() {
        let now = SystemTime::now();
        let old = now
            .checked_sub(TTL + Duration::from_secs(60))
            .expect("no underflow");
        let age = EntryAge {
            modified: Some(old),
            changed: Some(now),
        };
        assert!(!is_expired(&age, TTL, now));
    }

    #[test]
    fn both_mtime_and_ctime_old_is_expired() {
        let now = SystemTime::now();
        let old = now
            .checked_sub(TTL + Duration::from_secs(60))
            .expect("no underflow");
        let age = EntryAge {
            modified: Some(old),
            changed: Some(old),
        };
        assert!(is_expired(&age, TTL, now));
    }

    #[test]
    fn both_fresh_is_not_expired() {
        let now = SystemTime::now();
        let age = EntryAge {
            modified: Some(now),
            changed: Some(now),
        };
        assert!(!is_expired(&age, TTL, now));
    }

    /// A platform/entry with no ctime signal (`changed: None`) falls back
    /// to mtime alone — the documented residual window (ADR-055).
    #[test]
    fn no_ctime_signal_falls_back_to_mtime_only() {
        let now = SystemTime::now();
        let old = now
            .checked_sub(TTL + Duration::from_secs(60))
            .expect("no underflow");
        let age = EntryAge {
            modified: Some(old),
            changed: None,
        };
        assert!(is_expired(&age, TTL, now));
    }

    #[cfg(unix)]
    #[test]
    fn inprogress_marker_prevents_reclamation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        let file = dir.path().join("report.pdf");
        std::fs::write(&file, b"partial").expect("write");
        std::fs::write(dir.path().join("report.pdf.inprogress"), b"").expect("write marker");

        let age_source = FakeEntryAgeSource::default()
            .with(&file, FakeEntryAgeSource::old(now, Duration::from_secs(1)));

        let report =
            sweep_once_with_age_source(dir.path(), TTL, now, &age_source, &AtomicBool::new(false))
                .expect("sweep succeeds");

        assert!(
            file.exists(),
            "a file with an .inprogress marker must never be reclaimed"
        );
        assert_eq!(report.skipped, 1);
        assert_eq!(report.reclaimed_bytes, 0);
    }

    #[test]
    fn cancellation_stops_the_pass_before_visiting_every_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();
        const TOTAL: usize = 50;
        const CANCEL_AFTER: usize = 5;
        for i in 0..TOTAL {
            std::fs::write(dir.path().join(format!("f{i}.bin")), b"x").expect("write");
        }

        struct CancelAfterN<'a> {
            visited: AtomicUsize,
            threshold: usize,
            cancelled: &'a AtomicBool,
        }

        impl EntryAgeSource for CancelAfterN<'_> {
            fn age_of(&self, _path: &Path, _metadata: &std::fs::Metadata) -> EntryAge {
                if self.visited.fetch_add(1, Ordering::Relaxed) + 1 >= self.threshold {
                    self.cancelled.store(true, Ordering::Relaxed);
                }
                EntryAge::default()
            }
        }

        let cancelled = AtomicBool::new(false);
        let age_source = CancelAfterN {
            visited: AtomicUsize::new(0),
            threshold: CANCEL_AFTER,
            cancelled: &cancelled,
        };

        let report = sweep_once_with_age_source(dir.path(), TTL, now, &age_source, &cancelled)
            .expect("sweep succeeds");

        assert!(
            (report.scanned as usize) < TOTAL,
            "cancellation must stop the pass before every entry is visited, scanned={}",
            report.scanned
        );
        assert!(
            (report.scanned as usize) >= CANCEL_AFTER,
            "the pass must not stop before the cancellation signal is observed, scanned={}",
            report.scanned
        );
    }
}
