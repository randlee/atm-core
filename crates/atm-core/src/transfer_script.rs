//! Cross-host transfer-script resolution and the argv-array invocation
//! contract (ADR-055 decision (c)).
//!
//! This module resolves `~/.atm/transfer/<host>` (or `<host>.ps1` on
//! Windows), runs the executable-bit/owner-uid/not-group-or-other-accessible
//! safety check (ADR-055's widened `mode & 0o077` rule), and builds the
//! argv-array invocation. It also exposes
//! [`synthesized_transfer_script_env`], the deliberately minimal `PATH`
//! (plus, on Windows, a small set of process-startup variables) a spawning
//! caller sets on the child alongside [`TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`]
//! -- never the caller's own `PATH`. It deliberately does **not** execute
//! anything itself: spawning the resolved script is the CLI-surface lane's
//! job (`atm send --attach`), which lane C explicitly excludes. See
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
///
/// `ATM_TRANSFER_SSH_CONFIG` is an opt-in fourth entry (QA-2 B6): unset for
/// every ordinary operator (identical behavior to the original three-entry
/// allow-list), it exists so `sftp.sh`/`sftp.ps1` can be pointed at an
/// `ssh -F <path>` config file without the caller ever needing to touch a
/// real `~/.ssh/config` -- the seam `scripts/phase-aq/
/// run_aq4_transfer_evidence.py`'s live-evidence harness uses to route a
/// loopback `sshd` through a scratch config instead of mutating the OS
/// account's real one.
pub const TRANSFER_SCRIPT_ALLOWED_ENV_KEYS: [&str; 4] = [
    "ATM_TEMP",
    "ATM_IDENTITY",
    "ATM_TEAM",
    "ATM_TRANSFER_SSH_CONFIG",
];

/// Default bounded deadline for one transfer-script invocation before the
/// child is killed.
pub const DEFAULT_TRANSFER_SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Synthesizes the additional environment variables a transfer-script
/// child needs beyond [`TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`] to actually run
/// (ADR-055 decision (c) amendment, "PATH synthesis").
///
/// The allow-list above never included `PATH`, which is correct -- forwarding
/// the caller's real `PATH` to a script would leak whatever a developer's
/// shell profile happens to have on it, exactly the ambient-authority leak
/// the allow-list already refuses for every other variable. But `env_clear`
/// followed by *no* `PATH` at all left the shipped examples unable to
/// resolve `ssh`/`scp` at all on Windows (clean-runner CI, run
/// 33135390308): unlike POSIX `execvp`, which falls back to a
/// `confstr(_CS_PATH)`-provided default search path when `PATH` is entirely
/// absent from the child's environment -- the reason the Unix leg happened
/// to keep working -- Windows has no such fallback for a plain command
/// lookup performed by the *child* process itself (as opposed to the
/// initial `CreateProcess` search for `pwsh.exe`, which uses the *calling*
/// process's own `PATH`, not the argument this function builds).
///
/// The fix is a **synthesized**, deliberately narrow `PATH`, never the
/// caller's own: `/usr/bin:/bin:/usr/local/bin` (`+/opt/homebrew/bin` on
/// macOS) on Unix, and `%SystemRoot%\System32;%SystemRoot%\System32\
/// OpenSSH` plus `pwsh`'s own directory (best-effort, located via the
/// caller's `PATH` purely to find where `pwsh` lives -- that lookup value
/// is never forwarded onward as a whole) on Windows. Windows additionally
/// gets `SystemRoot`/`SYSTEMROOT` (the .NET/PowerShell host needs one of
/// these to start at all) and, when the caller has one, `TEMP` (`pwsh`'s
/// own temp-file needs) -- sourced from `env` rather than hardcoded, so an
/// operator's real Windows install layout is respected without ever
/// forwarding their full `PATH`.
///
/// A second amendment (2026-08-27, AQ4 run 33144153970) widens this on both
/// platforms with the user's identity/profile-location variables, forwarded
/// from `env` only when the caller actually has them set: `USERPROFILE`,
/// `HOMEDRIVE`, `HOMEPATH`, `LOCALAPPDATA`, `APPDATA`, `ProgramData`, and
/// `COMSPEC` on Windows, and `HOME` on Unix. These are never secrets and
/// never `PATH` -- they are the same class of "where do I live" variable as
/// `SystemRoot`/`TEMP` above, just the ones Windows OpenSSH's *own* home
/// resolution needs rather than `pwsh`'s. Windows `ssh.exe` resolves the
/// invoking user's home directory (`~/.ssh/config`, `known_hosts`, the
/// default identity file) via `USERPROFILE`/`HOMEDRIVE`/`HOMEPATH` even when
/// an explicit `-F <config>` is given, unlike Unix `ssh`, which resolves the
/// same information via `getpwuid()` and therefore never needed `HOME` in
/// the child's environment at all -- the asymmetry this amendment closes.
/// `pwsh` itself additionally consults `LOCALAPPDATA`/`APPDATA` for its own
/// profile/module paths, and without them can fail before the script's own
/// logic -- and its diagnostics -- ever run (run 33144153970: an env-clear
/// child died silently, while the same `ssh -F <config>` invocation run from
/// this harness's own full-environment Python process succeeded outright).
/// `ProgramData` and `COMSPEC` are carried for the same reason: they are
/// process-startup variables some Windows toolchains (and PowerShell's own
/// native-command handling, which shells out via `COMSPEC` in some code
/// paths) assume are present, and forwarding them costs nothing since they
/// never carry secrets.
///
/// Callers apply every returned pair with `Command::env` alongside (never
/// instead of) the [`TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`] allow-list, after
/// `Command::env_clear()`.
#[cfg(windows)]
#[must_use]
pub fn synthesized_transfer_script_env(env: &dyn EnvSource) -> Vec<(&'static str, OsString)> {
    windows_synthesized_transfer_script_env(env)
}

/// Callers apply every returned pair with `Command::env` alongside (never
/// instead of) the [`TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`] allow-list, after
/// `Command::env_clear()`. `env` is consulted for `HOME` (forwarded only
/// when the caller has it set) -- see the second amendment on
/// [`synthesized_transfer_script_env`]'s Windows sibling above for the
/// symmetry rationale; a fixed `PATH` is always present regardless.
#[cfg(not(windows))]
#[must_use]
pub fn synthesized_transfer_script_env(env: &dyn EnvSource) -> Vec<(&'static str, OsString)> {
    let mut vars = vec![("PATH", unix_synthesized_transfer_script_path())];
    if let Some(home) = env.var("HOME") {
        vars.push(("HOME", OsString::from(home)));
    }
    vars
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unix_synthesized_transfer_script_path() -> OsString {
    OsString::from("/usr/bin:/bin:/usr/local/bin")
}

#[cfg(target_os = "macos")]
fn unix_synthesized_transfer_script_path() -> OsString {
    OsString::from("/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
}

#[cfg(windows)]
fn windows_synthesized_transfer_script_env(env: &dyn EnvSource) -> Vec<(&'static str, OsString)> {
    let system_root = env
        .var("SystemRoot")
        .or_else(|| env.var("SYSTEMROOT"))
        .unwrap_or_else(|| r"C:\Windows".to_string());
    let mut vars = vec![
        (
            "PATH",
            windows_synthesized_transfer_script_path(&system_root, env),
        ),
        ("SystemRoot", OsString::from(system_root.clone())),
        ("SYSTEMROOT", OsString::from(system_root)),
    ];
    if let Some(temp) = env.var("TEMP").or_else(|| env.var("TMP")) {
        vars.push(("TEMP", OsString::from(temp)));
    }
    // Identity/profile-location variables (second amendment, 2026-08-27):
    // forwarded only when the caller actually has them, never hardcoded.
    // `USERPROFILE`/`HOMEDRIVE`/`HOMEPATH` are what Windows OpenSSH's own
    // home-directory resolution reads (`~/.ssh/config`, `known_hosts`, the
    // default identity), even with an explicit `-F <config>`; `LOCALAPPDATA`/
    // `APPDATA` are `pwsh`'s own profile/module-path needs; `ProgramData`
    // and `COMSPEC` are common Windows process-startup assumptions. None of
    // these carry secrets -- they are locations, not credentials.
    for key in [
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "LOCALAPPDATA",
        "APPDATA",
        "ProgramData",
        "COMSPEC",
    ] {
        if let Some(value) = env.var(key) {
            vars.push((key, OsString::from(value)));
        }
    }
    vars
}

/// Builds the Windows synthesized `PATH` value: the OpenSSH client
/// directories the shipped `sftp.ps1` example needs, plus `pwsh`'s own
/// directory when it can be located.
#[cfg(windows)]
fn windows_synthesized_transfer_script_path(system_root: &str, env: &dyn EnvSource) -> OsString {
    let mut parts = vec![
        format!(r"{system_root}\System32"),
        format!(r"{system_root}\System32\OpenSSH"),
    ];
    if let Some(pwsh_dir) = locate_pwsh_dir(env) {
        parts.push(pwsh_dir.display().to_string());
    }
    OsString::from(parts.join(";"))
}

/// Best-effort lookup of the directory containing `pwsh.exe`, searched
/// through the caller's own `PATH` (read once, here only, purely to find
/// this one directory -- the caller's `PATH` value itself is never
/// forwarded to the child). Returns `None` when `pwsh.exe` cannot be found
/// on any `PATH` entry; the resulting synthesized `PATH` still contains the
/// OpenSSH directories either way.
#[cfg(windows)]
fn locate_pwsh_dir(env: &dyn EnvSource) -> Option<PathBuf> {
    let path_var = env.var("PATH")?;
    std::env::split_paths(&path_var).find_map(|dir| {
        let candidate = dir.join("pwsh.exe");
        candidate.is_file().then_some(dir)
    })
}

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

/// Cross-crate test seam over [`resolve_transfer_script_in`]: lets the `atm`
/// CLI crate's transfer-invocation tests build a real, resolver-validated
/// [`ConfiguredTransferScript`] fixture against a throwaway directory,
/// without touching the real `$HOME`/`~/.atm/transfer` and without this
/// crate exposing `ConfiguredTransferScript`'s private fields.
///
/// # Errors
///
/// Returns [`AtmTempError`] under the same conditions as
/// [`resolve_transfer_script`].
#[cfg(any(test, feature = "test-utils"))]
pub fn resolve_transfer_script_in_for_tests(
    transfer_root: &Path,
    host: &HostName,
) -> Result<TransferScript, AtmTempError> {
    resolve_transfer_script_in(transfer_root, host)
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
        &canonicalize_for_containment(transfer_root),
        &canonicalize_for_containment(&profile_home),
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
        &canonicalize_for_containment(path),
        &canonicalize_for_containment(&profile_home),
        metadata.file_type().is_symlink(),
        "the transfer script",
        host,
    )
}

/// Resolves `path` to its canonical form for the Windows containment check
/// specifically (QM43 windows-CI regression): `Path::canonicalize()` on
/// Windows resolves symlinks/junctions **and** normalizes an 8.3
/// short-name path segment (e.g. `RUNNER~1`) to its long-name equivalent
/// (`runneradmin`) -- both real, observed aliasing sources for the *same*
/// filesystem location that a raw `Path::components()` comparison cannot
/// see through on its own (this is exactly what broke on the
/// `windows-latest` GitHub Actions runner: `%TEMP%` resolved through the
/// short-name alias while `%USERPROFILE%` resolved through the long name,
/// so an in-profile temp directory was wrongly rejected as
/// "outside the profile"). Falls back to the as-given path on failure (a
/// nonexistent or inaccessible path fails toward the stricter
/// containment-rejection branch in `check_path_within_profile`, not a
/// panic or a silently skipped check).
///
/// Applied only in this real-I/O wrapper, never inside
/// `check_path_within_profile` itself: canonicalizing requires the path to
/// exist on disk, which would make the pure core untestable with the
/// synthetic, nonexistent paths its own unit tests use.
#[cfg(windows)]
fn canonicalize_for_containment(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
///
/// **What this function does *not* do, and why that ordering matters.**
/// This pure core never canonicalizes anything itself (no filesystem I/O
/// of its own, by design) — it compares whatever `path`/`profile_home`
/// strings it is given. Two representations of the *same real location*
/// (an 8.3 short name like `RUNNER~1` versus its long-name equivalent
/// `runneradmin`, observed for real on the `windows-latest` GitHub Actions
/// runner: `%TEMP%` resolved through the short-name alias while
/// `%USERPROFILE%` resolved through the long name) will therefore compare
/// as *not* matching here, even though they refer to the same directory.
/// Resolving that is the caller's job: the `#[cfg(windows)]` wrapper
/// (`check_transfer_root_metadata`/`check_script_safety`) canonicalizes
/// both `path` and `profile_home` (`canonicalize_for_containment`, which
/// resolves 8.3 aliases the same way it resolves symlinks/junctions)
/// *before* calling this function — but only for the containment compare.
/// The reparse-point check must still run against the **un-canonicalized**
/// path (`symlink_metadata`'s `is_reparse_point`, computed by the caller
/// before it ever canonicalizes anything): canonicalizing a reparse point
/// *follows* it, which would defeat the exact check meant to catch one.
/// That is why `is_reparse_point` arrives here as an already-computed,
/// independent `bool` rather than being derived from `path` itself.
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

    /// Documents the contract `check_path_within_profile`'s own doc
    /// comment states: the pure core does **not** resolve 8.3
    /// short-name-vs-long-name aliasing (`RUNNER~1` vs `runneradmin`) on
    /// its own -- two strings naming the *same real directory* in
    /// different forms compare as unrelated here. This is exactly the
    /// shape of the `windows-latest` CI regression this test module's
    /// sibling test (`eight_dot_three_short_name_alias_is_accepted_after_canonicalization`,
    /// `#[cfg(windows)]`-only, since it needs a real 8.3 alias) reproduces
    /// end to end through the canonicalizing wrapper; this test pins the
    /// pure core's side of the contract on every platform, so a future
    /// change that quietly makes the pure core "smarter" about aliasing
    /// without updating its doc comment gets caught here.
    #[test]
    fn short_name_and_long_name_forms_of_the_same_directory_are_not_merged_by_the_pure_core() {
        let profile_home = PathBuf::from(r"C:\Users\runneradmin");
        let path = PathBuf::from(r"C:\Users\RUNNER~1\AppData\Local\Temp\.tmpY0TZeT");
        let error = check_path_within_profile(
            &path,
            &profile_home,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect_err(
            "the pure core must not silently merge a short-name and long-name spelling of the \
             same directory -- resolving that is the canonicalizing wrapper's job",
        );
        assert!(
            matches!(&error, AtmTempError::TransferScriptUnsafe { reason, .. } if reason.contains("outside")),
            "{error:?}"
        );
    }

    /// Reproduces the `windows-latest` CI regression end to end through the
    /// real canonicalizing wrapper (`canonicalize_for_containment`): a
    /// path reached through its 8.3 short-name alias must still be
    /// accepted as within the profile once both sides are canonicalized,
    /// because `Path::canonicalize()` on Windows resolves a short-name
    /// segment to its long-name equivalent the same way it resolves a
    /// symlink or junction.
    ///
    /// Some Windows volumes/images have 8.3 name generation disabled
    /// (`NtfsDisable8dot3NameCreation=1`), in which case `GetShortPathNameW`
    /// returns the long name unchanged and there is nothing to prove here;
    /// this test skips gracefully rather than asserting a false failure.
    #[cfg(windows)]
    #[test]
    fn eight_dot_three_short_name_alias_is_accepted_after_canonicalization() {
        let parent = tempfile::tempdir().expect("tempdir");
        let long_name_dir = parent
            .path()
            .join("a-sufficiently-long-directory-name-for-8dot3-generation");
        std::fs::create_dir(&long_name_dir).expect("create long-name dir");

        let Some(short_name_dir) = windows_short_path_name(&long_name_dir) else {
            eprintln!(
                "skipping eight_dot_three_short_name_alias_is_accepted_after_canonicalization: \
                 could not query a short-name alias"
            );
            return;
        };
        if short_name_dir == long_name_dir {
            eprintln!(
                "skipping eight_dot_three_short_name_alias_is_accepted_after_canonicalization: \
                 8.3 short-name generation is disabled on this volume"
            );
            return;
        }

        std::fs::create_dir_all(long_name_dir.join(".atm").join("transfer"))
            .expect("create nested target dir");

        // Mirrors the CI bug exactly: the checked path arrives through its
        // short-name alias (like `%TEMP%` did on the affected runner),
        // while `profile_home` is the long name (like `%USERPROFILE%`).
        let checked_path = short_name_dir.join(".atm").join("transfer");
        let canonical_path = canonicalize_for_containment(&checked_path);
        let canonical_profile = canonicalize_for_containment(&long_name_dir);
        check_path_within_profile(
            &canonical_path,
            &canonical_profile,
            false,
            "the ~/.atm/transfer directory",
            &host("m5"),
        )
        .expect("a short-name-aliased path under the profile must be accepted once canonicalized");
    }

    /// Looks up `path`'s 8.3 short-name alias via `GetShortPathNameW`.
    /// Returns `None` on any API failure (missing path, buffer issue) so
    /// the caller can skip its test gracefully rather than panicking on an
    /// environment-specific Windows API quirk.
    #[cfg(windows)]
    fn windows_short_path_name(path: &Path) -> Option<PathBuf> {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut buffer = vec![0u16; 4096];
        // SAFETY: `wide` is a valid NUL-terminated UTF-16 string for the
        // input path; `buffer` is a valid, writable buffer whose exact
        // element count is passed as `cchBuffer`.
        let length =
            unsafe { GetShortPathNameW(wide.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 || length as usize >= buffer.len() {
            return None;
        }
        buffer.truncate(length as usize);
        Some(PathBuf::from(OsString::from_wide(&buffer)))
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

    struct FakeEnvSource(std::collections::HashMap<&'static str, String>);

    impl EnvSource for FakeEnvSource {
        fn var(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
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
        vars.insert("HOME", "test-home-value".to_string());
        let env = FakeEnvSource(vars);
        let home = home_dir_from_env(&env).expect("HOME is set");
        assert_eq!(home, PathBuf::from("test-home-value"));
    }

    #[test]
    fn userprofile_is_used_when_home_is_unset() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("USERPROFILE", r"C:\Users\rand".to_string());
        let env = FakeEnvSource(vars);
        let home = home_dir_from_env(&env).expect("USERPROFILE is set");
        assert_eq!(home, PathBuf::from(r"C:\Users\rand"));
    }

    fn path_entry(entries: &[(&'static str, OsString)], key: &str) -> Option<OsString> {
        entries
            .iter()
            .find(|(entry_key, _)| *entry_key == key)
            .map(|(_, value)| value.clone())
    }

    /// ADR-055 decision (c) amendment (AQ4 Windows regression, run
    /// 33135390308): non-Windows platforms get exactly one synthesized
    /// entry, a fixed, deliberately narrow `PATH` -- never the caller's own,
    /// which this function's `env` parameter is not even consulted for on
    /// this platform (see `unix_synthesized_transfer_script_path`).
    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn unix_synthesized_env_is_exactly_the_fixed_minimal_path() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        let entries = synthesized_transfer_script_env(&env);
        assert_eq!(
            entries,
            vec![("PATH", OsString::from("/usr/bin:/bin:/usr/local/bin"))]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_synthesized_env_is_exactly_the_fixed_minimal_path() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        let entries = synthesized_transfer_script_env(&env);
        assert_eq!(
            entries,
            vec![(
                "PATH",
                OsString::from("/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
            )]
        );
    }

    /// Even though the current Unix implementation does not read `env` at
    /// all, this proves that contract at the call-site level rather than by
    /// inspecting the implementation: a distinctive caller `PATH` must never
    /// appear anywhere in the synthesized result.
    #[cfg(unix)]
    #[test]
    fn unix_synthesized_env_never_forwards_the_callers_path() {
        let mut vars = std::collections::HashMap::new();
        vars.insert(
            "PATH",
            "/definitely-not-a-real-dir/atm-caller-path-marker".to_string(),
        );
        let env = FakeEnvSource(vars);
        let entries = synthesized_transfer_script_env(&env);
        let path = path_entry(&entries, "PATH").expect("PATH is always present");
        assert!(
            !path.to_string_lossy().contains("atm-caller-path-marker"),
            "caller PATH must never leak: {path:?}"
        );
    }

    /// Second amendment (2026-08-27, AQ4 run 33144153970): `HOME` is
    /// forwarded only when the caller actually has it set, mirroring the
    /// Windows `TEMP` test below -- never hardcoded, never present when the
    /// caller's environment doesn't have it either.
    #[cfg(unix)]
    #[test]
    fn unix_synthesized_env_forwards_home_only_when_the_caller_has_one() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        assert_eq!(
            path_entry(&synthesized_transfer_script_env(&env), "HOME"),
            None
        );

        // A platform-neutral placeholder (matching `home_env_var_is_used_
        // when_present` above): this test only proves `HOME`'s raw value
        // flows through unchanged, not that it looks like a real Unix home
        // directory.
        let mut vars = std::collections::HashMap::new();
        vars.insert("HOME", "test-home-value".to_string());
        let env = FakeEnvSource(vars);
        assert_eq!(
            path_entry(&synthesized_transfer_script_env(&env), "HOME"),
            Some(OsString::from("test-home-value"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_synthesized_env_defaults_system_root_when_unset() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        let entries = synthesized_transfer_script_env(&env);
        let path = path_entry(&entries, "PATH").expect("PATH is always present");
        let path = path.to_string_lossy();
        assert!(path.contains(r"C:\Windows\System32"), "{path}");
        assert!(path.contains(r"C:\Windows\System32\OpenSSH"), "{path}");
        assert_eq!(
            path_entry(&entries, "SystemRoot"),
            Some(OsString::from(r"C:\Windows"))
        );
        assert_eq!(
            path_entry(&entries, "SYSTEMROOT"),
            Some(OsString::from(r"C:\Windows"))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_synthesized_env_uses_an_explicit_system_root_and_never_forwards_the_callers_path() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("SystemRoot", r"D:\CustomWindows".to_string());
        vars.insert(
            "PATH",
            r"C:\definitely-not-a-real-dir\atm-caller-path-marker".to_string(),
        );
        let env = FakeEnvSource(vars);
        let entries = synthesized_transfer_script_env(&env);
        let path = path_entry(&entries, "PATH").expect("PATH is always present");
        let path = path.to_string_lossy();
        assert!(
            path.starts_with(r"D:\CustomWindows\System32;D:\CustomWindows\System32\OpenSSH"),
            "{path}"
        );
        assert!(
            !path.contains("atm-caller-path-marker"),
            "caller PATH must never leak: {path}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_synthesized_env_includes_pwshs_own_directory_when_locatable() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("pwsh.exe"), b"").expect("write fake pwsh.exe");
        let mut vars = std::collections::HashMap::new();
        vars.insert(
            "PATH",
            dir.path().to_str().expect("utf8 tempdir path").to_string(),
        );
        let env = FakeEnvSource(vars);
        let entries = synthesized_transfer_script_env(&env);
        let path = path_entry(&entries, "PATH").expect("PATH is always present");
        assert!(
            path.to_string_lossy()
                .contains(dir.path().to_str().expect("utf8 tempdir path")),
            "expected pwsh's own directory in the synthesized PATH: {path:?}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_synthesized_env_forwards_temp_only_when_the_caller_has_one() {
        let env = FakeEnvSource(std::collections::HashMap::new());
        assert_eq!(
            path_entry(&synthesized_transfer_script_env(&env), "TEMP"),
            None
        );

        let mut vars = std::collections::HashMap::new();
        vars.insert("TEMP", r"C:\Users\rand\AppData\Local\Temp".to_string());
        let env = FakeEnvSource(vars);
        assert_eq!(
            path_entry(&synthesized_transfer_script_env(&env), "TEMP"),
            Some(OsString::from(r"C:\Users\rand\AppData\Local\Temp"))
        );
    }

    /// Second amendment (2026-08-27, AQ4 run 33144153970): the identity/
    /// profile-location variables Windows OpenSSH and `pwsh` need to resolve
    /// the invoking user's home directory (`USERPROFILE`, `HOMEDRIVE`,
    /// `HOMEPATH`, `LOCALAPPDATA`, `APPDATA`, `ProgramData`, `COMSPEC`) are
    /// each forwarded only when the caller actually has them set -- never
    /// hardcoded, and absent entirely when the caller's environment doesn't
    /// have them either.
    #[cfg(windows)]
    #[test]
    fn windows_synthesized_env_forwards_profile_identity_vars_only_when_the_caller_has_them() {
        let keys = [
            "USERPROFILE",
            "HOMEDRIVE",
            "HOMEPATH",
            "LOCALAPPDATA",
            "APPDATA",
            "ProgramData",
            "COMSPEC",
        ];

        let env = FakeEnvSource(std::collections::HashMap::new());
        let entries = synthesized_transfer_script_env(&env);
        for key in keys {
            assert_eq!(
                path_entry(&entries, key),
                None,
                "{key} must be absent when the caller doesn't have it"
            );
        }

        let mut vars = std::collections::HashMap::new();
        for key in keys {
            vars.insert(key, format!("value-for-{key}"));
        }
        let env = FakeEnvSource(vars);
        let entries = synthesized_transfer_script_env(&env);
        for key in keys {
            assert_eq!(
                path_entry(&entries, key),
                Some(OsString::from(format!("value-for-{key}"))),
                "{key} must be forwarded unchanged when the caller has it"
            );
        }
    }
}
