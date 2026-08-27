//! Cross-host transfer-script resolution and the argv-array invocation
//! contract (ADR-055 decision (c)).
//!
//! This module resolves `~/.atm/transfer/<host>` (or `<host>.ps1` on
//! Windows), runs the executable-bit/owner-uid/not-group-or-world-writable
//! safety check, and builds the argv-array invocation. It deliberately does
//! **not** execute anything: spawning the resolved script is the CLI-surface
//! lane's job (`atm send --attach`), which lane C explicitly excludes. See
//! `docs/adr/ADR-055-atm-temp-and-transfer-seam.md` for the full design
//! rationale.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ulid::Ulid;

use crate::atm_temp::AtmTempError;
use crate::home::resolve_user_home;
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
    let home = resolve_user_home().map_err(|_| AtmTempError::Unresolvable)?;
    Ok(home.join(".atm").join("transfer"))
}

/// Testable core of [`resolve_transfer_script`]: resolves against an
/// explicit transfer-script root instead of the real home directory, so
/// tests never touch `$HOME`.
fn resolve_transfer_script_in(
    transfer_root: &Path,
    host: &HostName,
) -> Result<TransferScript, AtmTempError> {
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
        Err(_) => Ok(TransferScript::NotConfigured { host: host.clone() }),
    }
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
            host: host.as_str().to_string(),
            reason: format!("{} is not a regular file", path.display()),
        });
    }
    let mode = metadata.permissions().mode();
    if !is_owner_executable(mode) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.as_str().to_string(),
            reason: "script is not owner-executable".to_string(),
        });
    }
    let owner_uid = current_process_uid();
    if !is_owned_by_uid(metadata.uid(), owner_uid) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.as_str().to_string(),
            reason: format!(
                "script is owned by uid {} instead of the current uid {owner_uid}",
                metadata.uid()
            ),
        });
    }
    if is_group_or_world_writable(mode) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.as_str().to_string(),
            reason: format!(
                "script permissions {:04o} are group- or world-writable",
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
            host: host.as_str().to_string(),
            reason: format!("{} is not a regular file", path.display()),
        });
    }
    // Windows has no POSIX exec-bit/owner-uid model here; ADR-055 does not
    // add a Windows-specific ACL check for the transfer script.
    Ok(())
}

/// Pure predicate so tests can exercise "not owner-executable" without
/// depending on how the mode bits were produced.
#[cfg(unix)]
fn is_owner_executable(mode: u32) -> bool {
    mode & 0o100 != 0
}

/// Pure predicate so tests can exercise "foreign owner" by asserting
/// against a deliberately wrong uid, without needing a second real account.
#[cfg(unix)]
fn is_owned_by_uid(actual_uid: u32, expected_uid: u32) -> bool {
    actual_uid == expected_uid
}

/// Pure predicate for the group/world-writable refusal reason.
#[cfg(unix)]
fn is_group_or_world_writable(mode: u32) -> bool {
    mode & 0o022 != 0
}

#[cfg(unix)]
fn current_process_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions.
    unsafe { libc::geteuid() }
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
        let resolved = resolve_transfer_script_in(dir.path(), &host("m5")).expect("resolves");
        assert_eq!(resolved, TransferScript::NotConfigured { host: host("m5") });
    }

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
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("writable")),
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
                matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("writable")),
                "{error:?}"
            );
        }

        #[test]
        fn is_owned_by_uid_rejects_a_foreign_owner() {
            assert!(is_owned_by_uid(501, 501));
            assert!(!is_owned_by_uid(501, 502));
        }

        #[test]
        fn is_owner_executable_requires_the_owner_bit() {
            assert!(is_owner_executable(0o700));
            assert!(!is_owner_executable(0o600));
        }

        #[test]
        fn is_group_or_world_writable_detects_either_bit() {
            assert!(!is_group_or_world_writable(0o700));
            assert!(is_group_or_world_writable(0o720));
            assert!(is_group_or_world_writable(0o702));
        }
    }
}
