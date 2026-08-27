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
    let profile_home =
        windows_profile_home_for_containment(&ProcessEnvSource).ok_or_else(|| {
            AtmTempError::TransferScriptUnsafe {
                host: host.clone(),
                reason: "could not resolve a home directory to validate the ~/.atm/transfer \
                      directory's containment ($HOME/%USERPROFILE% unset and the OS account \
                      profile is unavailable)"
                    .to_string(),
            }
        })?;
    check_path_within_profile(
        transfer_root,
        &profile_home,
        metadata.file_type().is_symlink(),
        "the ~/.atm/transfer directory",
        host,
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
    let profile_home =
        windows_profile_home_for_containment(&ProcessEnvSource).ok_or_else(|| {
            AtmTempError::TransferScriptUnsafe {
                host: host.clone(),
                reason: "could not resolve a home directory to validate the transfer \
                      script's containment ($HOME/%USERPROFILE% unset and the OS account \
                      profile is unavailable)"
                    .to_string(),
            }
        })?;
    check_path_within_profile(
        path,
        &profile_home,
        metadata.file_type().is_symlink(),
        "the transfer script",
        host,
    )
}

/// Resolves the home directory Windows path-containment checks
/// (`check_path_within_profile`, below) validate against: the **same**
/// `$HOME`-then-`%USERPROFILE%` precedence `transfer_script_root` uses to
/// build the transfer root in the first place
/// (`crate::home::resolve_user_home_via`), never re-derived independently.
/// Validating containment against a *different* resolution (the earlier
/// version of this check used `crate::home::os_account_home`, which
/// ignores an explicit `$HOME`/`%USERPROFILE%` override) is exactly what
/// let a legitimate override get rejected: `transfer_script_root` would
/// build the transfer root under the override, and this check would then
/// refuse it for sitting outside the *unrelated* OS-account profile.
///
/// Falls back to `crate::home::os_account_home` (the known-folder API)
/// only when neither environment variable is set. Returns `None` (fail
/// closed, never silently skipping the containment check) when neither
/// source resolves.
#[cfg(windows)]
fn windows_profile_home_for_containment(env: &dyn EnvSource) -> Option<PathBuf> {
    resolve_profile_home(
        crate::home::resolve_user_home_via(env),
        crate::home::os_account_home().ok(),
    )
}

/// Pure fallback-order decision behind [`windows_profile_home_for_containment`],
/// extracted so it is unit-testable on every platform without touching the
/// real environment or the Windows known-folder API: prefer `from_env` (the
/// override-aware resolution), fall back to `from_os_account` only when
/// `from_env` is `None`, and fail closed (`None`) when both are `None`.
#[cfg(any(windows, test))]
fn resolve_profile_home(
    from_env: Option<PathBuf>,
    from_os_account: Option<PathBuf>,
) -> Option<PathBuf> {
    from_env.or(from_os_account)
}

/// Pure, platform-independent core of the Windows transfer-script path
/// safety check. Compiled and unit-tested on every platform, not only
/// Windows, because it performs no filesystem or WinAPI I/O of its own:
/// callers supply `is_reparse_point` (from an already-taken
/// `symlink_metadata`, never re-derived by following the path) and
/// `profile_home` (see [`windows_profile_home_for_containment`]) as plain
/// data.
///
/// Refuses a reparse point outright: an NTFS symlink or junction could
/// point somewhere outside `profile_home` by the time it is actually used,
/// defeating everything below it.
///
/// Otherwise requires `path` to sit under `profile_home`, compared
/// **component-by-component** (`Path::components()`), not by raw string
/// prefix, for two reasons:
/// - a sibling directory sharing a string prefix must not pass —
///   `C:\Users\rand` is not a path-*component* prefix of
///   `C:\Users\randlee` (`rand` != `randlee` as the differing final shared
///   segment), even though it is a raw *string* prefix of it;
/// - a verbatim (`\\?\`) or UNC path-prefix component must not spuriously
///   mismatch a non-verbatim prefix naming the same location (a real,
///   easy-to-hit gap when one side has been `canonicalize()`d and the
///   other has not).
///
/// **Explicitly deferred, not silently assumed safe:** this does **not**
/// inspect the file/directory's Windows ACL (who else besides the current
/// user has write access). Unlike Unix mode bits, a Windows ACL has no
/// single-comparison shape to check generically; mirroring `atm_temp.rs`'s
/// own Windows scratch-root branch (`validate_existing_scratch_dir`, which
/// also performs no ACL check and documents why), this ships the
/// achievable minimum bar now and records the gap here rather than
/// pretending it is closed. See ADR-055's Windows-safety amendment for the
/// full rationale.
#[cfg(any(windows, test))]
fn check_path_within_profile(
    path: &Path,
    profile_home: &Path,
    is_reparse_point: bool,
    what: &str,
    host: &HostName,
) -> Result<(), AtmTempError> {
    if is_reparse_point {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "{what} at {} is a reparse point (symlink or junction); refusing to trust its target",
                path.display()
            ),
        });
    }
    if !path_is_within(path, profile_home) {
        return Err(AtmTempError::TransferScriptUnsafe {
            host: host.clone(),
            reason: format!(
                "{what} at {} is outside the current user's profile directory ({})",
                path.display(),
                profile_home.display()
            ),
        });
    }
    Ok(())
}

/// Component-aware "is `path` located under `root`" check. Unlike
/// `Path::starts_with`, a leading path-prefix component (a Windows drive
/// letter or UNC server/share) is normalized before comparison
/// (`normalized_prefix_key`), so a verbatim (`\\?\C:\`) and non-verbatim
/// (`C:\`) spelling of the same prefix compare equal; every other
/// component still compares exactly, so a sibling directory sharing a
/// string prefix (`rand` vs `randlee`) is correctly rejected.
#[cfg(any(windows, test))]
fn path_is_within(path: &Path, root: &Path) -> bool {
    let mut path_components = path.components();
    for root_component in root.components() {
        let Some(path_component) = path_components.next() else {
            return false;
        };
        if !components_match(root_component, path_component) {
            return false;
        }
    }
    true
}

#[cfg(any(windows, test))]
fn components_match(expected: std::path::Component<'_>, actual: std::path::Component<'_>) -> bool {
    use std::path::Component;

    match (expected, actual) {
        (Component::Prefix(expected), Component::Prefix(actual)) => {
            normalized_prefix_key(expected.kind()) == normalized_prefix_key(actual.kind())
        }
        (Component::RootDir, Component::RootDir)
        | (Component::CurDir, Component::CurDir)
        | (Component::ParentDir, Component::ParentDir) => true,
        (Component::Normal(expected), Component::Normal(actual)) => expected == actual,
        _ => false,
    }
}

/// A path-prefix component's logical identity, normalized so a verbatim
/// (`\\?\C:\`, `\\?\UNC\server\share`) and non-verbatim (`C:\`,
/// `\\server\share`) spelling of the same drive or UNC location produce
/// the same key. Windows drive letters and UNC server/share names are
/// case-insensitive, so both are uppercased.
#[cfg(any(windows, test))]
fn normalized_prefix_key(kind: std::path::Prefix<'_>) -> String {
    use std::path::Prefix;

    match kind {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("disk:{}", (letter as char).to_ascii_uppercase())
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
            "unc:{}:{}",
            server.to_string_lossy().to_ascii_uppercase(),
            share.to_string_lossy().to_ascii_uppercase()
        ),
        Prefix::Verbatim(component) => format!("verbatim:{}", component.to_string_lossy()),
        Prefix::DeviceNS(component) => format!("devicens:{}", component.to_string_lossy()),
    }
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

    // These tests exercise `check_path_within_profile`/`path_is_within`/
    // `resolve_profile_home` -- the pure, platform-independent core behind
    // the Windows transfer-script path safety check. They are compiled and
    // run on every CI platform (not gated `#[cfg(windows)]`) because the
    // functions under test perform no filesystem or WinAPI I/O of their
    // own; all paths below are synthetic and never touch a real
    // filesystem.

    #[test]
    fn path_within_profile_is_accepted() {
        let profile_home = PathBuf::from("C:/Users/randlee");
        let path = PathBuf::from("C:/Users/randlee/.atm/transfer");
        check_path_within_profile(
            &path,
            &profile_home,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect("a path under the profile home must be accepted");
    }

    /// The classic path-prefix bug: `C:\Users\randlee` shares a raw
    /// *string* prefix with `C:\Users\rand`, but it is a **sibling**
    /// directory, not a subdirectory -- a naive `str::starts_with`-style
    /// check would wrongly accept it. `check_path_within_profile` compares
    /// path *components*, so `rand` != `randlee` as the differing final
    /// shared segment correctly refuses it.
    #[test]
    fn sibling_prefix_directory_is_rejected() {
        let profile_home = PathBuf::from("C:/Users/rand");
        let path = PathBuf::from("C:/Users/randlee/.atm/transfer");
        let error = check_path_within_profile(
            &path,
            &profile_home,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect_err("a sibling directory sharing a string prefix must be refused");
        assert!(
            matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("outside")),
            "{error:?}"
        );
    }

    #[test]
    fn reparse_point_is_rejected_even_when_nominally_inside_the_profile() {
        let profile_home = PathBuf::from("C:/Users/randlee");
        let path = PathBuf::from("C:/Users/randlee/.atm/transfer");
        let error = check_path_within_profile(
            &path,
            &profile_home,
            true,
            "the transfer script",
            &host("m5"),
        )
        .expect_err("a reparse point must be refused regardless of its nominal location");
        assert!(
            matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("reparse point")),
            "{error:?}"
        );
    }

    /// A verbatim (`\\?\`) drive prefix and its non-verbatim spelling name
    /// the same location and must compare equal, not mismatch because one
    /// side was `canonicalize()`d and the other was not.
    ///
    /// `#[cfg(windows)]`-only: `Component::Prefix` is produced only by
    /// Windows path parsing (`std::path::Component`'s prefix variant has
    /// no meaning for a Unix path, which has no drive letters or UNC
    /// roots at all -- a backslash is not even a separator there), so
    /// this specific scenario cannot be exercised on a non-Windows
    /// target regardless of how the input is constructed. Every other
    /// test in this group is genuinely cross-platform.
    #[cfg(windows)]
    #[test]
    fn verbatim_drive_prefix_matches_its_non_verbatim_spelling() {
        let profile_home = PathBuf::from(r"\\?\C:\Users\randlee");
        let path = PathBuf::from(r"C:\Users\randlee\.atm\transfer");
        check_path_within_profile(
            &path,
            &profile_home,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect(
            "a verbatim profile prefix must match a non-verbatim path prefix for the same drive",
        );
    }

    /// `resolve_profile_home` (the pure fallback-order decision behind
    /// `windows_profile_home_for_containment`): an env-resolved override
    /// wins over the OS-account fallback when both are available -- this
    /// is what makes an explicit `$HOME`/`%USERPROFILE%` override honored
    /// end to end, since `transfer_script_root` resolves the transfer root
    /// itself through the exact same env-first precedence.
    #[test]
    fn resolve_profile_home_prefers_the_env_override_over_the_os_account_fallback() {
        let overridden = PathBuf::from("D:/CustomHome");
        let os_account_default = PathBuf::from("C:/Users/randlee");
        assert_eq!(
            resolve_profile_home(Some(overridden.clone()), Some(os_account_default)),
            Some(overridden)
        );
    }

    #[test]
    fn resolve_profile_home_falls_back_to_os_account_when_no_override_is_set() {
        let os_account_default = PathBuf::from("C:/Users/randlee");
        assert_eq!(
            resolve_profile_home(None, Some(os_account_default.clone())),
            Some(os_account_default)
        );
    }

    #[test]
    fn resolve_profile_home_fails_closed_when_neither_source_resolves() {
        assert_eq!(resolve_profile_home(None, None), None);
    }

    /// End-to-end proof (still through the pure core only) that an
    /// overridden home is honored: a path under the *override* is
    /// accepted even though it sits nowhere near the OS-account default.
    #[test]
    fn override_home_is_honored_for_containment() {
        let overridden = PathBuf::from("D:/CustomHome");
        let os_account_default = PathBuf::from("C:/Users/randlee");
        let profile_home = resolve_profile_home(Some(overridden.clone()), Some(os_account_default))
            .expect("override resolves");
        let path = overridden.join(".atm").join("transfer");
        check_path_within_profile(
            &path,
            &profile_home,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect("a path under the honored override home must be accepted");
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
