//! Cross-host transfer-script resolution and the argv-array invocation
//! contract (ADR-055 decision (c)).
//!
//! This module resolves `~/.atm/transfer/<host>` (or `<host>.ps1` on
//! Windows), runs the executable-bit/owner-uid/not-group-or-other-accessible
//! safety check (ADR-055's widened `mode & 0o077` rule), and builds the
//! argv-array invocation. It deliberately does **not** execute anything:
//! spawning the resolved script is the CLI-surface lane's job (`atm send
//! --attach`), which lane C explicitly excludes. See
//! `docs/adr/ADR-055-atm-temp-and-transfer-seam.md` for the full design
//! rationale.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ulid::Ulid;

use crate::atm_temp::{AtmTempError, EnvSource, ProcessEnvSource};
#[cfg(unix)]
use crate::atm_temp::{current_uid, is_owned_by_uid};
use crate::types::HostName;

/// The child-process environment a transfer script inherits: an explicit
/// allow-list, never the full parent environment (ADR-055 decision (c)).
pub const TRANSFER_SCRIPT_ALLOWED_ENV_KEYS: [&str; 3] = ["ATM_TEMP", "ATM_IDENTITY", "ATM_TEAM"];

/// Default bounded deadline for one transfer-script invocation before the
/// child is killed.
pub const DEFAULT_TRANSFER_SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Outcome of resolving `~/.atm/transfer/<host>`.
///
/// A missing script is not itself an error: [`resolve_transfer_script`]
/// returns `NotConfigured` so the caller can surface the canonical
/// `File transfer to <host> not enabled...` setup message (owned by the
/// CLI/send surface, not this module). Only an existing-but-unsafe script is
/// an [`AtmTempError::TransferScriptUnsafe`] error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferScript {
    /// No script file exists for this host.
    NotConfigured { host: HostName },
    /// A resolved script that passed the safety check and is ready to
    /// invoke.
    Configured(ConfiguredTransferScript),
}

/// How a resolved transfer script must be exec'd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferScriptKind {
    /// The script itself is the exec program (macOS/Linux).
    Direct,
    /// The script is invoked as `pwsh -File <script> ...` (Windows).
    PowerShell,
}

/// A transfer script that resolved and passed the safety check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredTransferScript {
    host: HostName,
    script_path: PathBuf,
    kind: TransferScriptKind,
}

impl ConfiguredTransferScript {
    #[must_use]
    pub fn host(&self) -> &HostName {
        &self.host
    }

    #[must_use]
    pub fn script_path(&self) -> &Path {
        &self.script_path
    }

    #[must_use]
    pub fn kind(&self) -> TransferScriptKind {
        self.kind
    }

    /// Builds the argv-array invocation for one transfer: `program` is the
    /// process to exec, `args` are its arguments in order. Never
    /// shell-interpolated — every argument is passed as a distinct argv
    /// element, never joined into a command string.
    #[must_use]
    pub fn invocation(&self, transfer_id: Ulid, files: &[PathBuf]) -> TransferInvocation {
        let mut args: Vec<OsString> = Vec::with_capacity(files.len() + 4);
        if matches!(self.kind, TransferScriptKind::PowerShell) {
            args.push(OsString::from("-File"));
            args.push(self.script_path.clone().into_os_string());
        }
        args.push(OsString::from(self.host.as_str()));
        args.push(OsString::from(transfer_id.to_string()));
        args.extend(files.iter().map(|file| file.clone().into_os_string()));
        let program = match self.kind {
            TransferScriptKind::Direct => self.script_path.clone(),
            TransferScriptKind::PowerShell => PathBuf::from("pwsh"),
        };
        TransferInvocation { program, args }
    }
}

/// An argv-array child-process invocation: `program` is the exec target,
/// `args` its ordered arguments. Building this is the whole point of
/// [`ConfiguredTransferScript::invocation`] — no caller ever assembles a
/// shell command string from these parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferInvocation {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// Resolves and safety-checks the transfer script for `host` under the
/// real user's home directory (`~/.atm/transfer/`).
///
/// # Errors
///
/// Returns [`AtmTempError::TransferScriptUnsafe`] when a script exists but
/// fails the owner-executable / owner-uid / not-group-or-world-writable
/// check. A missing script is not an error — see [`TransferScript`].
pub fn resolve_transfer_script(host: &HostName) -> Result<TransferScript, AtmTempError> {
    let transfer_root = transfer_script_root()?;
    resolve_transfer_script_in(&transfer_root, host)
}

fn transfer_script_root() -> Result<PathBuf, AtmTempError> {
    home_dir_from_env(&ProcessEnvSource).map(|home| home.join(".atm").join("transfer"))
}

/// Resolves the user's home directory through the [`EnvSource`] seam, so a
/// missing-home-directory failure is unit-testable with a `FakeEnvSource`
/// instead of mutating the real process environment.
///
/// Delegates the actual `$HOME`-then-`%USERPROFILE%` precedence decision to
/// [`crate::home::resolve_user_home_via`] (RBQA-F001) rather than
/// reimplementing it here: this function's only job is mapping that shared
/// precedence's `None` to this module's own error type.
///
/// # Errors
///
/// Returns [`AtmTempError::HomeDirUnavailable`] when neither variable is
/// set (or both are empty) — distinct from [`AtmTempError::Unresolvable`],
/// which names an `ATM_TEMP`-resolution failure, not a home-directory one.
fn home_dir_from_env(env: &dyn EnvSource) -> Result<PathBuf, AtmTempError> {
    crate::home::resolve_user_home_via(env).ok_or(AtmTempError::HomeDirUnavailable)
}

/// Testable core of [`resolve_transfer_script`]: resolves against an
/// explicit transfer-script root instead of the real home directory, so
/// tests never touch `$HOME`.
fn resolve_transfer_script_in(
    transfer_root: &Path,
    host: &HostName,
) -> Result<TransferScript, AtmTempError> {
    check_transfer_root_safety(transfer_root, host)?;
    let (script_path, kind) = script_path_for(transfer_root, host);
    match std::fs::symlink_metadata(&script_path) {
        Ok(metadata) => {
            check_script_safety(&script_path, host, &metadata)?;
            Ok(TransferScript::Configured(ConfiguredTransferScript {
                host: host.clone(),
                script_path,
                kind,
            }))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(TransferScript::NotConfigured { host: host.clone() })
        }
        Err(error) => {
            tracing::warn!(
                subsystem = "transfer_script",
                action = "resolve",
                outcome = "metadata_read_failed",
                host = host.as_str(),
                path = %script_path.display(),
                %error,
                "failed to read transfer-script metadata; refusing to treat this the same as \"not configured\""
            );
            Err(AtmTempError::TransferScriptUnreadable {
                host: host.clone(),
                reason: error.to_string(),
            })
        }
    }
}

/// Safety-checks `~/.atm/transfer` itself (QM43-I8): the transfer-script
/// safety check previously validated only the script file's own owner/mode,
/// never the containing directory's -- combined with a check-by-path/
/// exec-by-path pattern, that left a TOCTOU window if the directory itself
/// were writable by another local principal (they could replace a script
/// between this check and exec, or plant one under a host name that has
/// never been configured yet). A missing directory is not an error here --
/// see [`TransferScript::NotConfigured`] -- the subsequent script lookup
/// reports that case.
///
/// # Errors
///
/// Returns [`AtmTempError::TransferScriptUnsafe`] when the directory exists
/// but is not owned by the current uid, has any group/other permission bit
/// set, or is not a directory at all -- with a reason distinct from the
/// script-level checks so an operator is pointed at `~/.atm/transfer`
/// itself, not a specific host's script.
fn check_transfer_root_safety(transfer_root: &Path, host: &HostName) -> Result<(), AtmTempError> {
    match std::fs::symlink_metadata(transfer_root) {
        Ok(metadata) => check_transfer_root_metadata(transfer_root, host, &metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => {
            tracing::warn!(
                subsystem = "transfer_script",
                action = "resolve_root",
                outcome = "metadata_read_failed",
                host = host.as_str(),
                path = %transfer_root.display(),
                %error,
                "failed to read the ~/.atm/transfer directory's metadata"
            );
            Err(AtmTempError::TransferScriptUnreadable {
                host: host.clone(),
                reason: format!("~/.atm/transfer metadata could not be read: {error}"),
            })
        }
    }
}

#[cfg(unix)]
fn check_transfer_root_metadata(
    transfer_root: &Path,
    host: &HostName,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if !metadata.is_dir() {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!("{} is not a directory", transfer_root.display()),
        });
    }
    let owner_uid = current_uid();
    if !is_owned_by_uid(metadata, owner_uid) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "~/.atm/transfer is owned by uid {} instead of the current uid {owner_uid}",
                metadata.uid()
            ),
        });
    }
    let mode = metadata.permissions().mode();
    if crate::atm_temp::has_group_or_world_bits(mode) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "~/.atm/transfer permissions {:04o} grant group or other access; expected 0700 \
                 (owner-only read/write/execute)",
                mode & 0o777
            ),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn check_transfer_root_metadata(
    transfer_root: &Path,
    host: &HostName,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    if !metadata.is_dir() {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!("{} is not a directory", transfer_root.display()),
        });
    }
    check_windows_path_safety(
        transfer_root,
        host,
        metadata,
        "the ~/.atm/transfer directory",
    )
}

#[cfg(windows)]
fn script_path_for(transfer_root: &Path, host: &HostName) -> (PathBuf, TransferScriptKind) {
    (
        transfer_root.join(format!("{}.ps1", host.as_str())),
        TransferScriptKind::PowerShell,
    )
}

#[cfg(not(windows))]
fn script_path_for(transfer_root: &Path, host: &HostName) -> (PathBuf, TransferScriptKind) {
    (
        transfer_root.join(host.as_str()),
        TransferScriptKind::Direct,
    )
}

#[cfg(unix)]
fn check_script_safety(
    path: &Path,
    host: &HostName,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if !metadata.is_file() {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!("{} is not a regular file", path.display()),
        });
    }
    let mode = metadata.permissions().mode();
    if !is_owner_executable(mode) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: "script is not owner-executable".to_string(),
        });
    }
    let owner_uid = current_uid();
    if !is_owned_by_uid(metadata, owner_uid) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "script is owned by uid {} instead of the current uid {owner_uid}",
                metadata.uid()
            ),
        });
    }
    // ADR-055's widened rule: refuse any group/other permission bit at all
    // (`mode & 0o077 != 0`), not just group/other-writable. This is the same
    // shared-host bit-check `crate::atm_temp::has_group_or_world_bits` uses
    // for the `ATM_TEMP` scratch root, reused here rather than reinvented.
    if crate::atm_temp::has_group_or_world_bits(mode) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "script permissions {:04o} grant group or other access; expected 0700 \
                 (owner-only read/write/execute)",
                mode & 0o777
            ),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn check_script_safety(
    path: &Path,
    host: &HostName,
    metadata: &std::fs::Metadata,
) -> Result<(), AtmTempError> {
    if !metadata.is_file() {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!("{} is not a regular file", path.display()),
        });
    }
    check_windows_path_safety(path, host, metadata, "the transfer script")
}

/// Minimum-bar Windows safety check for the transfer-script seam
/// (`docs/cross-platform-guidelines.md`'s `#[cfg(windows)]`-split pattern),
/// applied to both the script file and its containing `~/.atm/transfer`
/// directory. Windows has no POSIX owner-uid/mode-bits model, so this is
/// not a like-for-like port of the Unix check; it verifies two structural
/// properties instead:
///
/// 1. `path` is not a reparse point (an NTFS symlink or junction) — a
///    reparse point passing this check could point somewhere outside the
///    validated location by the time it is used, defeating everything
///    below it. `metadata` comes from `symlink_metadata` (never followed),
///    so this is exactly the same "never follow symlinks out of the
///    checked location" discipline the rest of this module already uses.
/// 2. `path` resolves under the current OS account's profile directory,
///    read through the same known-folder API `crate::home::os_account_home`
///    uses for host-runtime ownership (`SHGetKnownFolderPath`), not
///    `%USERPROFILE%`, which a caller process can redirect.
///
/// **Explicitly deferred, not silently assumed safe:** this does **not**
/// inspect the file/directory's Windows ACL (who else has write access).
/// Unlike Unix mode bits, a Windows ACL has no single-comparison shape to
/// check generically; mirroring `atm_temp.rs`'s own Windows scratch-root
/// branch (`validate_existing_scratch_dir`, which also performs no ACL
/// check and documents why), this ships the achievable minimum bar now
/// and records the gap here rather than pretending it is closed.
#[cfg(windows)]
fn check_windows_path_safety(
    path: &Path,
    host: &HostName,
    metadata: &std::fs::Metadata,
    what: &'static str,
) -> Result<(), AtmTempError> {
    if metadata.file_type().is_symlink() {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "{what} at {} is a reparse point (symlink or junction); refusing to trust its target",
                path.display()
            ),
        });
    }
    let profile =
        crate::home::os_account_home().map_err(|_| AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "could not resolve the current user's profile directory to validate {what} at {}",
                path.display()
            ),
        })?;
    // Both sides canonicalized consistently: Windows `canonicalize()`
    // normalizes to a double-backslash-question-mark-prefixed
    // extended-length absolute path, and comparing one
    // canonicalized side against one non-canonicalized side would produce
    // spurious `starts_with` mismatches. Falling back to the
    // as-resolved path/profile on a canonicalize failure fails toward the
    // stricter branch below (an unresolvable path is unlikely to already
    // start with a resolvable profile path).
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_profile = profile.canonicalize().unwrap_or(profile);
    if !canonical_path.starts_with(&canonical_profile) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "{what} at {} is outside the current user's profile directory ({})",
                path.display(),
                canonical_profile.display()
            ),
        });
    }
    Ok(())
}

/// Pure predicate so tests can exercise "not owner-executable" without
/// depending on how the mode bits were produced.
#[cfg(unix)]
fn is_owner_executable(mode: u32) -> bool {
    mode & 0o100 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(value: &str) -> HostName {
        value.parse().expect("valid host")
    }

    #[test]
    fn missing_script_is_not_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        secure_dir_for_test(dir.path());
        let resolved = resolve_transfer_script_in(dir.path(), &host("m5")).expect("resolves");
        assert_eq!(resolved, TransferScript::NotConfigured { host: host("m5") });
    }

    /// `tempfile::tempdir()` does not guarantee mode `0700` on every
    /// platform/umask combination, but [`check_transfer_root_safety`]
    /// requires it (QM43-I8); tests that expect a successful resolution
    /// secure the directory explicitly first, exactly as a real
    /// `~/.atm/transfer` must be secured by its owner.
    #[cfg(unix)]
    fn secure_dir_for_test(dir: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .expect("chmod test transfer root");
    }

    #[cfg(not(unix))]
    fn secure_dir_for_test(_dir: &Path) {}

    #[cfg(not(windows))]
    #[test]
    fn direct_script_path_has_no_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, kind) = script_path_for(dir.path(), &host("m5"));
        assert_eq!(path, dir.path().join("m5"));
        assert_eq!(kind, TransferScriptKind::Direct);
    }

    #[cfg(windows)]
    #[test]
    fn windows_script_path_uses_ps1_extension() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (path, kind) = script_path_for(dir.path(), &host("m5"));
        assert_eq!(path, dir.path().join("m5.ps1"));
        assert_eq!(kind, TransferScriptKind::PowerShell);
    }

    #[test]
    fn invocation_builds_direct_argv_without_shell_interpolation() {
        let configured = ConfiguredTransferScript {
            host: host("m5"),
            script_path: PathBuf::from("/home/rand/.atm/transfer/m5"),
            kind: TransferScriptKind::Direct,
        };
        let transfer_id = Ulid::new();
        let files = vec![PathBuf::from("/tmp/report.pdf")];
        let invocation = configured.invocation(transfer_id, &files);
        assert_eq!(
            invocation.program,
            PathBuf::from("/home/rand/.atm/transfer/m5")
        );
        assert_eq!(
            invocation.args,
            vec![
                OsString::from("m5"),
                OsString::from(transfer_id.to_string()),
                OsString::from("/tmp/report.pdf"),
            ]
        );
    }

    #[test]
    fn invocation_builds_powershell_argv_with_file_flag() {
        let configured = ConfiguredTransferScript {
            host: host("m5"),
            script_path: PathBuf::from(r"C:\Users\rand\.atm\transfer\m5.ps1"),
            kind: TransferScriptKind::PowerShell,
        };
        let transfer_id = Ulid::new();
        let files = vec![PathBuf::from(r"C:\staging\report.pdf")];
        let invocation = configured.invocation(transfer_id, &files);
        assert_eq!(invocation.program, PathBuf::from("pwsh"));
        assert_eq!(
            invocation.args,
            vec![
                OsString::from("-File"),
                OsString::from(r"C:\Users\rand\.atm\transfer\m5.ps1"),
                OsString::from("m5"),
                OsString::from(transfer_id.to_string()),
                OsString::from(r"C:\staging\report.pdf"),
            ]
        );
    }

    #[cfg(unix)]
    mod unix_only {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn write_script(dir: &Path, name: &str, mode: u32) -> PathBuf {
            secure_dir_for_test(dir);
            let path = dir.join(name);
            std::fs::write(&path, "#!/bin/sh\necho ok\n").expect("write script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("chmod");
            path
        }

        #[test]
        fn owner_executable_own_uid_private_script_is_configured() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o700);
            let resolved = resolve_transfer_script_in(dir.path(), &host("m5")).expect("resolves");
            match resolved {
                TransferScript::Configured(configured) => {
                    assert_eq!(configured.kind(), TransferScriptKind::Direct);
                    assert_eq!(configured.script_path(), dir.path().join("m5"));
                }
                other => panic!("expected Configured, got {other:?}"),
            }
        }

        #[test]
        fn non_executable_script_is_unsafe() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o600);
            let error = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect_err("non-executable script must be refused");
            assert!(
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("owner-executable")),
                "{error:?}"
            );
        }

        #[test]
        fn group_writable_script_is_unsafe() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o770);
            let error = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect_err("group-writable script must be refused");
            assert!(
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("access")),
                "{error:?}"
            );
        }

        #[test]
        fn world_writable_script_is_unsafe() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o703);
            let error = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect_err("world-writable script must be refused");
            assert!(
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("access")),
                "{error:?}"
            );
        }

        /// ADR-055's widened check (`mode & 0o077`, QM43-B1): a script that is
        /// only group-readable/executable -- never writable -- must still be
        /// refused. The pre-widening check (`mode & 0o022`) would have
        /// accepted this.
        #[test]
        fn group_readable_and_executable_script_is_unsafe() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o750);
            let error = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect_err("group-readable/executable script must be refused");
            assert!(
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("access")),
                "{error:?}"
            );
        }

        #[test]
        fn is_owner_executable_requires_the_owner_bit() {
            assert!(is_owner_executable(0o700));
            assert!(!is_owner_executable(0o600));
        }

        /// A `symlink_metadata` failure other than "not found" (here:
        /// permission-denied on an ancestor directory) must not be
        /// collapsed into `NotConfigured` (QM43-I1): it is a distinct,
        /// propagated error.
        #[test]
        fn unreadable_ancestor_is_distinct_from_not_configured() {
            let dir = tempfile::tempdir().expect("tempdir");
            let locked_root = dir.path().join("locked");
            std::fs::create_dir(&locked_root).expect("create locked root");
            let script_path = locked_root.join("m5");
            std::fs::write(&script_path, "#!/bin/sh\necho ok\n").expect("write script");
            std::fs::set_permissions(&locked_root, std::fs::Permissions::from_mode(0o000))
                .expect("chmod locked root unreadable");

            let error = resolve_transfer_script_in(&locked_root, &host("m5"));

            std::fs::set_permissions(&locked_root, std::fs::Permissions::from_mode(0o700))
                .expect("restore locked root permissions");

            let error =
                error.expect_err("permission-denied ancestor must not read as NotConfigured");
            assert!(
                matches!(&error, AtmTempError::TransferScriptUnreadable { host, .. } if host.as_str() == "m5"),
                "{error:?}"
            );
        }

        /// QM43-I8: `~/.atm/transfer` itself must be owned by the caller
        /// and have no group/other permission bit, exactly like the
        /// scripts under it -- not just the script file's own mode.
        #[test]
        fn group_writable_transfer_root_is_unsafe() {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o770))
                .expect("chmod transfer root group-writable");
            write_script(dir.path(), "m5", 0o700);
            // `write_script` re-secures the directory to 0700 as a side
            // effect; put the insecure mode back after, so this test
            // actually exercises the directory-level refusal.
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o770))
                .expect("chmod transfer root group-writable");

            let error = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect_err("group-writable transfer root must be refused");
            assert!(
                matches!(
                    &error,
                    AtmTempError::TransferScriptUnsafe { reason, .. }
                        if reason.contains("~/.atm/transfer") && reason.contains("access")
                ),
                "{error:?}"
            );
        }

        /// QM43-I8 acceptance case: a properly-secured `0700`, own-uid
        /// transfer root with a safe script still resolves normally.
        #[test]
        fn owner_only_transfer_root_is_accepted() {
            let dir = tempfile::tempdir().expect("tempdir");
            write_script(dir.path(), "m5", 0o700);
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
                .expect("chmod transfer root owner-only");

            let resolved = resolve_transfer_script_in(dir.path(), &host("m5"))
                .expect("owner-only 0700 transfer root is accepted");
            assert!(
                matches!(resolved, TransferScript::Configured(_)),
                "{resolved:?}"
            );
        }
    }

    struct FakeEnvSource(std::collections::HashMap<&'static str, &'static str>);

    impl EnvSource for FakeEnvSource {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).map(|value| (*value).to_string())
        }
    }

    /// A home-directory resolution failure must surface as its own error,
    /// distinct from `AtmTempError::Unresolvable` (which names an
    /// `ATM_TEMP`-resolution failure, not a home-directory one) (QM43-B2).
    #[test]
    fn missing_home_env_vars_report_home_dir_unavailable() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        let error = home_dir_from_env(&env).expect_err("no HOME/USERPROFILE must fail closed");
        assert_eq!(error, AtmTempError::HomeDirUnavailable);
    }

    #[test]
    fn home_env_var_is_used_when_present() {
        // A platform-neutral placeholder: this test only proves `HOME`'s
        // raw value flows through unchanged, not that it looks like a real
        // Unix home directory (`home_dir_from_env` does not validate path
        // shape at all).
        let mut vars = std::collections::HashMap::new();
        vars.insert("HOME", "test-home-value");
        let env = FakeEnvSource(vars);
        let home = home_dir_from_env(&env).expect("HOME is set");
        assert_eq!(home, PathBuf::from("test-home-value"));
    }

    #[test]
    fn userprofile_is_used_when_home_is_unset() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("USERPROFILE", r"C:\Users\rand");
        let env = FakeEnvSource(vars);
        let home = home_dir_from_env(&env).expect("USERPROFILE is set");
        assert_eq!(home, PathBuf::from(r"C:\Users\rand"));
    }
}
