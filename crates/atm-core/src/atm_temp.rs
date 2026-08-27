//! `ATM_TEMP` scratch-root resolution and shared-host safety validation
//! (ADR-055).
//!
//! `resolve_atm_temp` is the single sanctioned `ATM_TEMP` read site: it
//! reads the variable through the [`EnvSource`] seam rather than calling
//! `std::env::var` directly, so production code, tests, and the
//! `env-var-boundary` lint all see exactly one call site. See
//! `docs/adr/ADR-055-atm-temp-and-transfer-seam.md` for the full design
//! rationale (non-breaking rollout, shared-host safety, sweep policy,
//! transfer-script seam).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Abstraction over environment-variable lookup.
///
/// The entire justification for this trait is that [`EnvSource::var`] is a
/// **method call**, never the free-function `env::var`/`std::env::var` path
/// form the `env-var-boundary` lint's `ENV_CALL_RE` matches (anchored on a
/// literal `::`). `resolve_atm_temp` is therefore lint-clean by
/// construction and needs no allowlist entry (ADR-055's "M14" note).
pub trait EnvSource {
    /// Returns the named environment variable's value, or `None` when it is
    /// unset or is not valid Unicode.
    fn var(&self, key: &str) -> Option<String>;
}

/// Reads from the real process environment.
///
/// The sole production implementation of [`EnvSource`]; every other
/// implementation exists so tests can inject deterministic values without
/// mutating the real process environment (which is process-global and
/// therefore unsafe to mutate from parallel tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessEnvSource;

impl EnvSource for ProcessEnvSource {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// A resolved, security-checked ATM scratch root.
///
/// Constructible only by [`resolve_atm_temp`]: this keeps "an `AtmTemp`
/// value exists" synonymous with "its directory passed the shared-host
/// safety check," so no caller can bypass the check by building one from an
/// arbitrary path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtmTemp(PathBuf);

impl AtmTemp {
    /// Returns the scratch root's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for AtmTemp {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

/// Failure modes for `ATM_TEMP` resolution and the transfer-script safety
/// check (ADR-055). Both share one error family because both are
/// "is this filesystem location safe to use" checks over the same
/// scratch-root contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtmTempError {
    /// `ATM_TEMP` was set to a relative path.
    NotAbsolute,
    /// The path (or an ancestor needed to create it) could not be resolved:
    /// a broken symlink, or a missing parent directory.
    Unresolvable,
    /// The resolved directory does not exist and could not be created, or
    /// exists but is not writable.
    NotWritable,
    /// The resolved directory (fallback default or explicit `ATM_TEMP`)
    /// exists but is not owned by the current uid, or has a group/world
    /// permission bit set.
    AtmTempInsecure { path: PathBuf, reason: String },
    /// A transfer script exists at `~/.atm/transfer/<host>` but failed the
    /// executable-bit / owner-uid / not-group-or-world-writable check.
    TransferScriptUnsafe { host: String, reason: String },
    /// The user's home directory could not be resolved (neither `$HOME` nor
    /// `%USERPROFILE%` is set) while locating `~/.atm/transfer`. Distinct
    /// from [`Self::Unresolvable`], which names an `ATM_TEMP`-resolution
    /// failure: this is a home-directory failure, unrelated to `ATM_TEMP`,
    /// and must not be reported with `ATM_TEMP`-flavored recovery text.
    HomeDirUnavailable,
    /// `~/.atm/transfer/<host>` could not be inspected for a reason other
    /// than "does not exist" (for example, a permission-denied ancestor
    /// directory, or a transient I/O error). Distinct from
    /// [`TransferScript::NotConfigured`](crate::transfer_script::TransferScript::NotConfigured):
    /// collapsing an unreadable-but-present path into "not configured" would
    /// point the operator at the wrong recovery action.
    TransferScriptUnreadable { host: String, reason: String },
}

impl fmt::Display for AtmTempError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAbsolute => f.write_str("ATM_TEMP must be an absolute path"),
            Self::Unresolvable => f.write_str(
                "ATM_TEMP could not be resolved (broken symlink or missing parent directory)",
            ),
            Self::NotWritable => f.write_str("ATM_TEMP is not writable and could not be created"),
            Self::AtmTempInsecure { path, reason } => {
                write!(
                    f,
                    "ATM_TEMP directory {} is insecure: {reason}",
                    path.display()
                )
            }
            Self::TransferScriptUnsafe { host, reason } => {
                write!(f, "transfer script for host '{host}' is unsafe: {reason}")
            }
            Self::HomeDirUnavailable => f.write_str(
                "the user's home directory could not be resolved (neither $HOME nor \
                 %USERPROFILE% is set); required to locate ~/.atm/transfer",
            ),
            Self::TransferScriptUnreadable { host, reason } => {
                write!(
                    f,
                    "transfer script for host '{host}' could not be read: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for AtmTempError {}

/// Unset resolves to a documented per-OS default (created if missing);
/// set-but-invalid fails closed. The daemon calls this once at startup; the
/// CLI calls it lazily at first scratch-space use. One function, one
/// default, one error family — the single `ATM_TEMP` read site (ADR-055).
///
/// # Errors
///
/// Returns [`AtmTempError`] when an explicit `ATM_TEMP` is relative,
/// unresolvable, or unwritable, or when the resolved directory (fallback or
/// explicit) fails the shared-host ownership/permission check.
pub fn resolve_atm_temp(env: &dyn EnvSource) -> Result<AtmTemp, AtmTempError> {
    match env.var("ATM_TEMP") {
        Some(raw) => resolve_explicit_atm_temp(&raw),
        None => resolve_at_root(default_scratch_root()),
    }
}

/// Reports whether resolution fell back to the default scratch root rather
/// than an explicit `ATM_TEMP`. Callers use this to decide whether to emit
/// the one-time fallback warning (ADR-055 decision (a)).
#[must_use]
pub fn is_atm_temp_unset(env: &dyn EnvSource) -> bool {
    env.var("ATM_TEMP").is_none()
}

/// The scratch root `resolve_atm_temp` uses today's process would resolve to
/// when `ATM_TEMP` is unset, formatted for the startup warning without
/// performing the (possibly filesystem-mutating) resolution itself.
#[must_use]
pub fn default_atm_temp_display() -> PathBuf {
    default_scratch_root()
}

/// `$ATM_TEMP/send-to/<transfer-id>/`: the shared staging-directory
/// convention every Send-To feature must use, so there is exactly one
/// construction site for this path shape.
#[must_use]
pub fn send_to_staging_dir(atm_temp: &AtmTemp, transfer_id: &ulid::Ulid) -> PathBuf {
    atm_temp
        .path()
        .join("send-to")
        .join(transfer_id.to_string())
}

fn resolve_explicit_atm_temp(raw: &str) -> Result<AtmTemp, AtmTempError> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(AtmTempError::NotAbsolute);
    }
    resolve_at_root(path)
}

/// Shared tail of both the explicit and default resolution paths: apply the
/// one shared-host safety check (ADR-055) to whichever root was selected.
/// Kept separate from `resolve_atm_temp` so tests can exercise the same
/// security-check code path the real default branch uses, against a
/// throwaway directory, without ever touching the real machine-wide
/// default scratch root (a path a real installed daemon may already own).
fn resolve_at_root(path: PathBuf) -> Result<AtmTemp, AtmTempError> {
    ensure_secure_scratch_dir(&path)?;
    Ok(AtmTemp(path))
}

fn ensure_secure_scratch_dir(path: &Path) -> Result<(), AtmTempError> {
    match std::fs::symlink_metadata(path) {
        Ok(link_metadata) if link_metadata.file_type().is_symlink() => {
            let target_metadata = std::fs::metadata(path).map_err(|error| {
                warn_atm_temp_io_error("resolve_symlink_target", path, &error);
                AtmTempError::Unresolvable
            })?;
            validate_existing_scratch_dir(path, &target_metadata)
        }
        Ok(metadata) => validate_existing_scratch_dir(path, &metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|candidate| !candidate.as_os_str().is_empty())
                .ok_or(AtmTempError::Unresolvable)?;
            parent.canonicalize().map_err(|error| {
                warn_atm_temp_io_error("canonicalize_parent", path, &error);
                AtmTempError::Unresolvable
            })?;
            create_scratch_dir(path)?;
            let metadata = std::fs::symlink_metadata(path).map_err(|error| {
                warn_atm_temp_io_error("stat_after_create", path, &error);
                AtmTempError::NotWritable
            })?;
            validate_existing_scratch_dir(path, &metadata)
        }
        Err(error) => {
            warn_atm_temp_io_error("stat_scratch_root", path, &error);
            Err(AtmTempError::NotWritable)
        }
    }
}

/// Logs a discarded I/O error at `warn` with structured fields before it is
/// mapped to a variant-only [`AtmTempError`] (`Unresolvable`/`NotWritable`
/// carry no `reason` field, unlike `AtmTempInsecure`/`TransferScriptUnsafe`,
/// so the underlying cause would otherwise vanish entirely rather than just
/// being coarsened).
fn warn_atm_temp_io_error(action: &'static str, path: &Path, error: &io::Error) {
    tracing::warn!(
        subsystem = "atm_temp",
        action,
        outcome = "io_error",
        path = %path.display(),
        %error,
        "ATM_TEMP scratch-root resolution hit an I/O error"
    );
}

#[cfg(unix)]
fn create_scratch_dir(path: &Path) -> Result<(), AtmTempError> {
    use std::os::unix::fs::DirBuilderExt;

    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|error| {
            warn_atm_temp_io_error("create_scratch_dir", path, &error);
            AtmTempError::NotWritable
        })
}

#[cfg(windows)]
fn create_scratch_dir(path: &Path) -> Result<(), AtmTempError> {
    std::fs::create_dir_all(path).map_err(|error| {
        warn_atm_temp_io_error("create_scratch_dir", path, &error);
        AtmTempError::NotWritable
    })
}

#[cfg(unix)]
fn validate_existing_scratch_dir(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if !metadata.is_dir() {
        return Err(AtmTempError::NotWritable);
    }
    let owner_uid = current_uid();
    if !is_owned_by_uid(metadata, owner_uid) {
        return Err(AtmTempError::AtmTempInsecure {
            path: path.to_path_buf(),
            reason: format!(
                "owned by uid {} instead of the current uid {owner_uid}",
                metadata.uid()
            ),
        });
    }
    let mode = metadata.permissions().mode();
    if has_group_or_world_bits(mode) {
        return Err(AtmTempError::AtmTempInsecure {
            path: path.to_path_buf(),
            reason: format!(
                "permissions {:04o} grant group or world access; expected 0700",
                mode & 0o777
            ),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn validate_existing_scratch_dir(
    _path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    if !metadata.is_dir() {
        return Err(AtmTempError::NotWritable);
    }
    // Windows has no POSIX owner/mode model here; `%TEMP%` is already
    // per-user, so ADR-055 does not add a Windows-specific ACL check.
    Ok(())
}

/// Returns whether `metadata`'s owner uid matches `owner_uid`. A pure
/// function so tests can exercise the "foreign owner" branch by asserting
/// against a deliberately wrong uid, without needing a second real account.
///
/// This is the single owner-uid check ADR-055 documents as shared: the
/// `ATM_TEMP` scratch-root check (below), the transfer-script safety check
/// (`crate::transfer_script::check_script_safety`), and the daemon's
/// UDS-socket owner check (`atm_http_runtime::unix_socket::is_owned_by`) all
/// call this one function rather than each inventing their own comparison.
#[cfg(unix)]
#[must_use]
pub fn is_owned_by_uid(metadata: &std::fs::Metadata, owner_uid: u32) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.uid() == owner_uid
}

/// Returns whether a Unix mode has any group or world permission bit set
/// (`mode & 0o077 != 0`): ADR-055's widened shared-host check, used both for
/// the `ATM_TEMP` scratch root (below) and the transfer-script safety check
/// (`crate::transfer_script::check_script_safety`), which the daemon's
/// UDS-socket precedent's narrower write-only `0o022` check does not cover.
#[cfg(unix)]
pub(crate) fn has_group_or_world_bits(mode: u32) -> bool {
    mode & 0o077 != 0
}

/// Returns the current process's effective uid. Shared by every ADR-055
/// owner-uid check (see [`is_owned_by_uid`]).
#[cfg(unix)]
#[must_use]
pub fn current_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions.
    unsafe { libc::geteuid() }
}

/// `<temp_dir>/atm-<uid>` — the Unix default scratch-root naming
/// convention. A pure function of its inputs so it can be unit tested
/// without touching the real process uid or `std::env::temp_dir()`.
/// Compiled on non-Unix test builds too so the naming convention stays
/// unit-testable on every CI host, not only a real Unix runner.
#[cfg(any(unix, test))]
fn unix_default_scratch_root(temp_dir: &Path, uid: u32) -> PathBuf {
    temp_dir.join(format!("atm-{uid}"))
}

/// `<temp_dir>\atm` — the Windows default scratch-root naming convention
/// (no uid suffix: `%TEMP%` is already per-user). Compiled on non-Windows
/// test builds too so this branch is testable on every CI host, not only a
/// real Windows runner.
#[cfg(any(windows, test))]
fn windows_default_scratch_root(temp_dir: &Path) -> PathBuf {
    temp_dir.join("atm")
}

#[cfg(unix)]
fn default_scratch_root() -> PathBuf {
    unix_default_scratch_root(&std::env::temp_dir(), current_uid())
}

#[cfg(windows)]
fn default_scratch_root() -> PathBuf {
    windows_default_scratch_root(&std::env::temp_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::capture_tracing;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeEnvSource(HashMap<String, String>);

    impl FakeEnvSource {
        fn unset() -> Self {
            Self::default()
        }

        fn with(key: &str, value: impl Into<String>) -> Self {
            let mut map = HashMap::new();
            map.insert(key.to_string(), value.into());
            Self(map)
        }
    }

    impl EnvSource for FakeEnvSource {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
    }

    #[test]
    fn unix_default_scratch_root_has_uid_suffix() {
        let root = unix_default_scratch_root(Path::new("/tmp"), 501);
        assert_eq!(root, Path::new("/tmp/atm-501"));
    }

    #[test]
    fn windows_default_scratch_root_has_no_uid_suffix() {
        let root = windows_default_scratch_root(Path::new("/tmp"));
        assert_eq!(root, Path::new("/tmp/atm"));
    }

    #[test]
    fn is_atm_temp_unset_reports_true_only_when_unset() {
        assert!(is_atm_temp_unset(&FakeEnvSource::unset()));
        assert!(!is_atm_temp_unset(&FakeEnvSource::with(
            "ATM_TEMP",
            "/anywhere"
        )));
    }

    #[test]
    fn explicit_relative_path_is_rejected() {
        let env = FakeEnvSource::with("ATM_TEMP", "relative/path");
        assert_eq!(resolve_atm_temp(&env), Err(AtmTempError::NotAbsolute));
    }

    #[test]
    fn explicit_path_with_missing_parent_is_unresolvable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist").join("atm-temp");
        let env = FakeEnvSource::with("ATM_TEMP", missing.to_str().expect("utf8 path"));
        assert_eq!(resolve_atm_temp(&env), Err(AtmTempError::Unresolvable));
    }

    #[test]
    fn send_to_staging_dir_uses_the_shared_convention() {
        let dir = tempfile::tempdir().expect("tempdir");
        let atm_temp = AtmTemp(dir.path().to_path_buf());
        let transfer_id = ulid::Ulid::new();
        let staged = send_to_staging_dir(&atm_temp, &transfer_id);
        assert_eq!(
            staged,
            dir.path().join("send-to").join(transfer_id.to_string())
        );
    }

    #[cfg(unix)]
    mod unix_only {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn explicit_new_path_is_created_with_mode_0700() {
            let dir = tempfile::tempdir().expect("tempdir");
            let target = dir.path().join("atm-temp");
            let env = FakeEnvSource::with("ATM_TEMP", target.to_str().expect("utf8 path"));
            let resolved = resolve_atm_temp(&env).expect("resolves");
            assert_eq!(resolved.path(), target);
            let mode = std::fs::metadata(&target)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }

        #[test]
        fn preexisting_0755_directory_is_refused_as_insecure() {
            let dir = tempfile::tempdir().expect("tempdir");
            let target = dir.path().join("atm-temp");
            std::fs::create_dir(&target).expect("create dir");
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
            let env = FakeEnvSource::with("ATM_TEMP", target.to_str().expect("utf8 path"));
            let error = resolve_atm_temp(&env).expect_err("insecure directory must be refused");
            assert!(
                matches!(error, AtmTempError::AtmTempInsecure { .. }),
                "{error:?}"
            );
        }

        #[test]
        fn preexisting_0700_own_uid_directory_is_accepted() {
            let dir = tempfile::tempdir().expect("tempdir");
            let target = dir.path().join("atm-temp");
            std::fs::create_dir(&target).expect("create dir");
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700))
                .expect("chmod");
            let env = FakeEnvSource::with("ATM_TEMP", target.to_str().expect("utf8 path"));
            let resolved = resolve_atm_temp(&env).expect("own-uid 0700 directory is accepted");
            assert_eq!(resolved.path(), target);
        }

        #[test]
        fn unset_atm_temp_routes_through_the_same_secure_creation_path() {
            // `resolve_atm_temp(unset)` calls `resolve_at_root(default_scratch_root())`
            // internally; exercising `resolve_at_root` directly against a
            // throwaway directory proves the same security-check code path
            // the real default branch uses, without ever touching the real
            // machine-wide default scratch root (a path a real installed
            // daemon may already own).
            let dir = tempfile::tempdir().expect("tempdir");
            let would_be_default_root = dir.path().join("would-be-default-root");
            let resolved = resolve_at_root(would_be_default_root.clone()).expect("resolves");
            assert_eq!(resolved.path(), would_be_default_root);
            let mode = std::fs::metadata(&would_be_default_root)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }

        #[test]
        fn default_atm_temp_display_matches_the_naming_convention() {
            let temp_dir = std::env::temp_dir();
            let expected = unix_default_scratch_root(&temp_dir, current_uid());
            assert_eq!(default_atm_temp_display(), expected);
        }

        #[test]
        fn is_owned_by_uid_rejects_a_foreign_owner() {
            let dir = tempfile::tempdir().expect("tempdir");
            let metadata = std::fs::metadata(dir.path()).expect("metadata");
            assert!(is_owned_by_uid(&metadata, current_uid()));
            assert!(!is_owned_by_uid(&metadata, current_uid().wrapping_add(1)));
        }

        #[test]
        fn has_group_or_world_bits_detects_any_extra_bit() {
            assert!(!has_group_or_world_bits(0o700));
            assert!(has_group_or_world_bits(0o701));
            assert!(has_group_or_world_bits(0o710));
            assert!(has_group_or_world_bits(0o740));
        }

        #[test]
        fn explicit_path_not_writable_reports_not_writable() {
            let dir = tempfile::tempdir().expect("tempdir");
            let parent = dir.path().join("locked-parent");
            std::fs::create_dir(&parent).expect("create parent");
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o500))
                .expect("chmod parent read-only");
            let target = parent.join("atm-temp");
            let env = FakeEnvSource::with("ATM_TEMP", target.to_str().expect("utf8 path"));
            let error = resolve_atm_temp(&env).expect_err("locked parent must refuse creation");
            assert_eq!(error, AtmTempError::NotWritable);
            // Restore permissions so tempfile can clean up the directory.
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
                .expect("restore parent permissions");
        }

        /// The underlying `io::Error` behind a `NotWritable`/`Unresolvable`
        /// result must not be silently discarded: it is logged at `warn`
        /// with structured `subsystem`/`action`/`outcome` fields (QM43-I2).
        #[test]
        fn not_writable_io_error_is_logged_not_discarded() {
            let dir = tempfile::tempdir().expect("tempdir");
            let parent = dir.path().join("locked-parent");
            std::fs::create_dir(&parent).expect("create parent");
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o500))
                .expect("chmod parent read-only");
            let target = parent.join("atm-temp");
            let env = FakeEnvSource::with("ATM_TEMP", target.to_str().expect("utf8 path"));

            let (result, logs) = capture_tracing(|| resolve_atm_temp(&env));
            let error = result.expect_err("locked parent must refuse creation");

            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
                .expect("restore parent permissions");

            assert_eq!(error, AtmTempError::NotWritable);
            assert!(
                logs.contains("subsystem=\"atm_temp\"")
                    && logs.contains("action=\"create_scratch_dir\"")
                    && logs.contains("outcome=\"io_error\""),
                "expected a structured warn for the discarded io::Error, got: {logs}"
            );
        }
    }
}
