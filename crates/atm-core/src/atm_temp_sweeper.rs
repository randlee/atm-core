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
use std::path::{Path, PathBuf};
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

/// One completed sweep pass's structured result.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Number of directory entries visited (files, directories, and
    /// symlinks alike).
    pub scanned: u64,
    /// Total bytes reclaimed by removing expired files and symlinks.
    pub reclaimed_bytes: u64,
    /// Number of entries left in place because of a per-entry failure
    /// (permission denied, a race with a concurrent writer, and so on).
    /// Never includes entries kept because they are still fresh.
    pub skipped: u64,
}

/// Removes everything under `root` whose own modification time is at least
/// `ttl` older than `now`.
///
/// Never follows a symlink into whatever it targets: a symlink entry is
/// aged and reclaimed using its own (`lstat`-observed) modification time,
/// exactly like any other entry, and its target is never visited. This
/// makes "never follows symlinks out of the root" true by construction.
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
    sweep_dir(root, ttl, now, &mut report);
    Ok(report)
}

fn sweep_dir(dir: &Path, ttl: Duration, now: SystemTime, report: &mut SweepReport) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        report.skipped += 1;
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else {
            report.skipped += 1;
            continue;
        };
        report.scanned += 1;
        let path = entry.path();
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            report.skipped += 1;
            continue;
        };
        let expired = is_expired(&metadata, ttl, now);

        if metadata.file_type().is_symlink() {
            if expired {
                reclaim_file(&path, &metadata, report);
            }
            continue;
        }

        if metadata.is_dir() {
            sweep_dir(&path, ttl, now, report);
            let now_empty = std::fs::read_dir(&path)
                .map(|mut remaining| remaining.next().is_none())
                .unwrap_or(false);
            if now_empty && expired && std::fs::remove_dir(&path).is_err() {
                report.skipped += 1;
            }
            continue;
        }

        if expired {
            reclaim_file(&path, &metadata, report);
        }
    }
}

fn reclaim_file(path: &Path, metadata: &std::fs::Metadata, report: &mut SweepReport) {
    let size = metadata.len();
    if std::fs::remove_file(path).is_ok() {
        report.reclaimed_bytes += size;
    } else {
        report.skipped += 1;
    }
}

fn is_expired(metadata: &std::fs::Metadata, ttl: Duration, now: SystemTime) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age >= ttl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

    fn age_file(path: &Path, age: Duration, now: SystemTime) {
        let modified = now
            .checked_sub(age)
            .expect("age must not underflow SystemTime");
        let file = std::fs::File::open(path).expect("open for mtime rewrite");
        file.set_modified(modified).expect("set mtime");
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
        age_file(&expired, TTL + Duration::from_secs(60), now);

        let fresh = dir.path().join("fresh.bin");
        std::fs::write(&fresh, b"01234").expect("write fresh");
        age_file(&fresh, Duration::from_secs(60), now);

        let report = sweep_once(dir.path(), TTL, now).expect("sweep succeeds");
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
        let nested = dir.path().join("send-to").join("01ABC");
        std::fs::create_dir_all(&nested).expect("nested dir");
        age_file(
            dir.path().join("send-to").as_path(),
            TTL + Duration::from_secs(1),
            now,
        );
        age_file(&nested, TTL + Duration::from_secs(1), now);

        let report = sweep_once(dir.path(), TTL, now).expect("sweep succeeds");
        assert!(report.scanned >= 2);
        assert!(!nested.exists());
        assert!(!dir.path().join("send-to").exists());
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

    #[cfg(unix)]
    #[test]
    fn skipped_counts_a_read_failure_without_aborting_the_pass() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let now = SystemTime::now();

        let ok_file = dir.path().join("ok.bin");
        std::fs::write(&ok_file, b"x").expect("write");
        age_file(&ok_file, TTL + Duration::from_secs(1), now);

        let locked_dir = dir.path().join("locked");
        std::fs::create_dir(&locked_dir).expect("create locked dir");
        age_file(&locked_dir, TTL + Duration::from_secs(1), now);
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o000))
            .expect("chmod locked dir unreadable");

        let report = sweep_once(dir.path(), TTL, now).expect("sweep succeeds despite one failure");

        // Restore permissions so `tempfile` can clean up the directory,
        // regardless of the assertions below.
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o700))
            .expect("restore locked dir permissions");

        assert_eq!(report.skipped, 1);
        assert!(
            !ok_file.exists(),
            "the unrelated expired file is still reclaimed"
        );
    }
}
