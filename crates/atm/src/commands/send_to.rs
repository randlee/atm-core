//! `atm send --attach`/`--from-json` CLI-surface plumbing (ADR-055
//! decisions (c)-(g)): the single CLI first-use `resolve_atm_temp` call
//! site, transfer-script invocation (the real process I/O the pure
//! `atm_core::send_to`/`atm_core::transfer_script` modules deliberately do
//! not perform), and the `--from-json` fan-out orchestration.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Once;
use std::time::Duration;

use anyhow::{Context, Result};
use atm_core::atm_temp::{
    AtmTemp, EnvSource, ProcessEnvSource, is_atm_temp_unset, resolve_atm_temp,
};
use atm_core::error::AtmError;
use atm_core::send_to::{
    RecipientLocality, stage_same_host_attachments, validate_landed_dir_stdout,
};
use atm_core::transfer_script::{
    ConfiguredTransferScript, TRANSFER_SCRIPT_ALLOWED_ENV_KEYS, TransferScript,
    resolve_transfer_script, synthesized_transfer_script_env,
};
use atm_core::types::HostName;
use ulid::Ulid;

/// Bytes captured from a transfer script's stdout/stderr before truncating
/// with a marker (ADR-055 decision (c): "capped stdout/stderr").
const MAX_CAPTURED_OUTPUT_BYTES: usize = 64 * 1024;
const TRUNCATION_MARKER: &str = "...[truncated]";

static ATM_TEMP_WARNING: Once = Once::new();

/// Resolves `$ATM_TEMP` at the CLI's first scratch-space touch (ADR-055
/// decision (a)): the single CLI-owned `resolve_atm_temp` call site.
///
/// Every `atm send --attach`/`--from-json` path that needs a scratch root
/// calls this instead of `atm_core::resolve_atm_temp` directly, so the
/// one-time fallback warning and the one-time-per-process semantics live in
/// exactly one place.
///
/// # Errors
///
/// Returns [`AtmError`] when an explicit `ATM_TEMP` is relative,
/// unresolvable, unwritable, or fails the shared-host ownership/permission
/// check (`AtmTempInsecure`) -- fails closed, per ADR-055.
pub(crate) fn resolve_atm_temp_for_cli() -> Result<AtmTemp, AtmError> {
    let env = ProcessEnvSource;
    let atm_temp = resolve_atm_temp(&env)?;
    if is_atm_temp_unset(&env) {
        ATM_TEMP_WARNING.call_once(|| {
            tracing::warn!(
                default_path = %atm_temp.path().display(),
                override_env = "ATM_TEMP",
                "ATM_TEMP is unset; using the default scratch root (set ATM_TEMP to override)"
            );
        });
    }
    Ok(atm_temp)
}

/// One `--attach` file lands either under this host's own staging directory
/// or a resolved remote host's transfer script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AttachmentLanding {
    pub(crate) landed_dir: PathBuf,
}

/// Stages or transfers `files` for one recipient locality (ADR-055
/// deliverable 3), returning where they landed.
///
/// # Errors
///
/// Returns an error when staging fails (same-host), the destination host has
/// no configured transfer script (the canonical "not enabled" setup error),
/// the script is unsafe to run, or the transfer itself fails or times out.
pub(crate) async fn land_attachments(
    atm_temp: &AtmTemp,
    transfer_id: Ulid,
    locality: &RecipientLocality,
    files: &[PathBuf],
) -> Result<AttachmentLanding> {
    match locality {
        RecipientLocality::SameHost => {
            let landed_dir = stage_same_host_attachments(atm_temp, &transfer_id, files)?;
            Ok(AttachmentLanding { landed_dir })
        }
        RecipientLocality::Remote(host) => {
            let landed_dir =
                resolve_and_invoke_transfer_script(&ProcessEnvSource, host, transfer_id, files)
                    .await?;
            Ok(AttachmentLanding { landed_dir })
        }
    }
}

/// The canonical, verbatim setup error for an unconfigured destination host
/// (ADR-055 decision (c)). Every caller that needs this exact text (CLI
/// error output, `docs/cross-host-file-transfer.md`) uses this one
/// constructor.
fn transfer_not_enabled_error(host: &HostName) -> AtmError {
    AtmError::validation_with_recovery(
        format!(
            "File transfer to {host} not enabled. Read docs/cross-host-file-transfer.md \
             to set up cross-host file transfer."
        ),
        format!(
            "Create an owner-only executable script at ~/.atm/transfer/{host} \
             (or {host}.ps1 on Windows) per docs/cross-host-file-transfer.md."
        ),
    )
}

async fn resolve_and_invoke_transfer_script(
    env: &dyn EnvSource,
    host: &HostName,
    transfer_id: Ulid,
    files: &[PathBuf],
) -> Result<PathBuf> {
    let script = resolve_transfer_script(host).map_err(AtmError::from)?;
    let configured = match script {
        TransferScript::Configured(configured) => configured,
        TransferScript::NotConfigured { host } => {
            return Err(transfer_not_enabled_error(&host).into());
        }
    };
    invoke_transfer_script(
        env,
        &configured,
        transfer_id,
        files,
        atm_core::transfer_script::DEFAULT_TRANSFER_SCRIPT_TIMEOUT,
    )
    .await
}

/// Spawns one bounded, argv-array transfer-script invocation (ADR-055
/// decision (c)).
///
/// The child inherits **only** [`TRANSFER_SCRIPT_ALLOWED_ENV_KEYS`] from this
/// process's environment (an explicit allow-list, never the full parent
/// environment), plus a deliberately minimal, synthesized `PATH` (and, on
/// Windows, a few process-startup variables) from
/// [`synthesized_transfer_script_env`] -- never the caller's own `PATH`
/// (ADR-055 decision (c) amendment). It has stdin closed, and is killed if
/// it outlives `timeout`. Captured stdout/stderr are capped at
/// [`MAX_CAPTURED_OUTPUT_BYTES`] with a truncation marker. Success is
/// validated as untrusted input: exactly one absolute-path line, no control
/// characters ([`validate_landed_dir_stdout`]).
///
/// # Errors
///
/// Returns an error when the child cannot be spawned, exits non-zero
/// (stderr, bounded, is included), is killed after exceeding `timeout`, or
/// produces stdout that fails [`validate_landed_dir_stdout`].
pub(crate) async fn invoke_transfer_script(
    env: &dyn EnvSource,
    configured: &ConfiguredTransferScript,
    transfer_id: Ulid,
    files: &[PathBuf],
    timeout: Duration,
) -> Result<PathBuf> {
    let invocation = configured.invocation(transfer_id, files);
    let mut command = tokio::process::Command::new(&invocation.program);
    command.args(&invocation.args);
    command.env_clear();
    for key in TRANSFER_SCRIPT_ALLOWED_ENV_KEYS {
        if let Some(value) = env.var(key) {
            command.env(key, value);
        }
    }
    for (key, value) in synthesized_transfer_script_env(env) {
        command.env(key, value);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to start transfer script for host '{}'",
            configured.host()
        )
    })?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stdout_task = tokio::spawn(read_capped(stdout));
    let stderr_task = tokio::spawn(read_capped(stderr));

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => status.with_context(|| {
            format!(
                "failed to wait for the transfer script for host '{}'",
                configured.host()
            )
        })?,
        Err(_elapsed) => {
            let _ = child.kill().await;
            return Err(AtmError::validation_with_recovery(
                format!(
                    "transfer script for host '{}' exceeded its {}s deadline and was killed",
                    configured.host(),
                    timeout.as_secs()
                ),
                "Speed up the script, or raise its configured deadline.",
            )
            .into());
        }
    };

    let stdout_bytes = stdout_task.await.unwrap_or_default();
    let stderr_bytes = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(AtmError::validation_with_recovery(
            format!(
                "transfer script for host '{}' failed ({status}): {}",
                configured.host(),
                String::from_utf8_lossy(&stderr_bytes)
            ),
            "Check the transfer script's stderr output above and retry.",
        )
        .into());
    }

    Ok(validate_landed_dir_stdout(&stdout_bytes)?)
}

/// Reads an async pipe up to [`MAX_CAPTURED_OUTPUT_BYTES`], appending
/// [`TRUNCATION_MARKER`] when more data remained. Never fails: a read error
/// is treated as "no more output" so a malfunctioning pipe cannot hang or
/// crash the caller.
async fn read_capped(mut reader: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    use tokio::io::AsyncReadExt;

    let mut buffer = vec![0_u8; MAX_CAPTURED_OUTPUT_BYTES];
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]).await {
            Ok(0) | Err(_) => break,
            Ok(read) => filled += read,
        }
    }
    buffer.truncate(filled);
    // Drain and discard anything beyond the cap so the child never blocks on
    // a full pipe buffer while this function is capping captured output.
    let mut discard = [0_u8; 4096];
    while let Ok(read) = reader.read(&mut discard).await {
        if read == 0 {
            break;
        }
    }
    if filled == MAX_CAPTURED_OUTPUT_BYTES {
        buffer.extend_from_slice(TRUNCATION_MARKER.as_bytes());
    }
    buffer
}

// Unix-only: every test below invokes a real `#!/bin/sh` transfer
// script directly (shebang exec, `sleep`, `env | cut`), which has no
// Windows equivalent, so the whole module is gated rather than each
// test individually.
//
// Every `invoke_transfer_script_*` test below is tagged
// `#[serial_test::serial(transfer_script_spawn)]`: each one forks and reaps
// a real child process (some killed mid-flight, e.g. the wedged-child
// deadline test), and `cargo test`'s default parallel-thread execution was
// observed to intermittently corrupt/stall sibling tests' captured
// stdout when several ran concurrently in the same process (CI run
// 33141901187: `invoke_transfer_script_rejects_multi_line_stdout` got an
// error whose text didn't match its own script's output; reproduced
// locally as a ~10% rate of 30s stalls on the wedged-child test *only*
// when run alongside its siblings, never in isolation). Serializing this
// module's real-subprocess tests removes the concurrent fork/reap window
// entirely rather than chasing the exact interleaving.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn host(value: &str) -> HostName {
        value.parse().expect("valid host")
    }

    fn write_script(dir: &std::path::Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, contents).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
        path
    }

    #[test]
    fn transfer_not_enabled_error_matches_the_canonical_text_verbatim() {
        let error = transfer_not_enabled_error(&host("m5"));
        assert_eq!(
            error.detail(),
            "File transfer to m5 not enabled. Read docs/cross-host-file-transfer.md \
             to set up cross-host file transfer."
        );
    }

    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_happy_path_returns_the_landed_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let landed = dir.path().join("landed");
        std::fs::create_dir_all(&landed).expect("landed dir");
        let script_path = write_script(
            dir.path(),
            "stub.sh",
            &format!("#!/bin/sh\necho {}\n", landed.display()),
        );
        let configured = fixture_configured_script(script_path, host("m5"));

        let result = invoke_transfer_script(
            &atm_core::test_support::FakeEnvSource::empty(),
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await
        .expect("happy path succeeds");

        assert_eq!(result, landed);
    }

    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_propagates_bounded_stderr_on_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = write_script(
            dir.path(),
            "stub.sh",
            "#!/bin/sh\necho boom over stderr >&2\nexit 1\n",
        );
        let configured = fixture_configured_script(script_path, host("m5"));

        let error = invoke_transfer_script(
            &atm_core::test_support::FakeEnvSource::empty(),
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await
        .expect_err("nonzero exit must fail");

        assert!(error.to_string().contains("boom over stderr"));
    }

    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_kills_a_wedged_child_after_its_deadline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = write_script(dir.path(), "stub.sh", "#!/bin/sh\nsleep 30\n");
        let configured = fixture_configured_script(script_path, host("m5"));

        let error = invoke_transfer_script(
            &atm_core::test_support::FakeEnvSource::empty(),
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_millis(200),
        )
        .await
        .expect_err("wedged script must be killed at the deadline");

        assert!(error.to_string().contains("deadline"));
    }

    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_rejects_multi_line_stdout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let script_path = write_script(dir.path(), "stub.sh", "#!/bin/sh\necho /one\necho /two\n");
        let configured = fixture_configured_script(script_path, host("m5"));

        let error = invoke_transfer_script(
            &atm_core::test_support::FakeEnvSource::empty(),
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await
        .expect_err("multi-line stdout must be rejected");

        assert!(error.to_string().contains("exactly one line"));
    }

    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_child_environment_is_restricted_to_the_allow_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let landed = dir.path().join("landed");
        std::fs::create_dir_all(&landed).expect("landed dir");
        // The capture path is baked into the script's own bytes (not passed
        // through the environment), so the allow-list restriction cannot
        // accidentally hide the very channel this test observes through.
        let capture_file = dir.path().join("env-capture.txt");
        let script_contents = format!(
            "#!/bin/sh\nenv | cut -d= -f1 | sort > {}\necho {}\n",
            capture_file.display(),
            landed.display()
        );
        let script_path = write_script(dir.path(), "stub.sh", &script_contents);
        let configured = fixture_configured_script(script_path, host("m5"));

        let env = atm_core::test_support::FakeEnvSource::new([
            ("ATM_TEMP", Some("atm-temp-test-value")),
            ("ATM_IDENTITY", Some("test-agent")),
            ("ATM_TEAM", Some("test-team")),
            ("NOT_ALLOWED_LEAK", Some("should-not-appear")),
        ]);

        let result = invoke_transfer_script(
            &env,
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await;

        result.expect("happy path succeeds");
        let captured =
            std::fs::read_to_string(&capture_file).expect("script wrote its captured env");
        let names: std::collections::HashSet<&str> = captured.lines().collect();
        // `/bin/sh` itself contributes a few shell-internal variables
        // (`PWD`, `SHLVL`, `_`) that were never in this process's
        // environment at all; the assertion below only cares about the
        // allow-list contract over variables this process actually held.
        assert!(names.contains("ATM_IDENTITY"));
        assert!(names.contains("ATM_TEAM"));
        assert!(names.contains("ATM_TEMP"));
        assert!(!names.contains("NOT_ALLOWED_LEAK"));
    }

    /// QA-2 B6: `ATM_TRANSFER_SSH_CONFIG` is an opt-in fourth allow-list
    /// entry -- forwarded when the caller's process happens to have it set
    /// (exactly like the other three), and simply absent from the child
    /// otherwise. Every ordinary `atm send --attach` invocation never sets
    /// it; only tooling like `scripts/phase-aq/run_aq4_transfer_evidence.py`
    /// does, to route `ssh`/`scp` through a scratch config instead of the
    /// real `~/.ssh/config`.
    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_forwards_the_opt_in_ssh_config_override_when_set() {
        let dir = tempfile::tempdir().expect("tempdir");
        let landed = dir.path().join("landed");
        std::fs::create_dir_all(&landed).expect("landed dir");
        let capture_file = dir.path().join("env-capture.txt");
        let script_contents = format!(
            "#!/bin/sh\nenv | grep '^ATM_TRANSFER_SSH_CONFIG=' > {}\necho {}\n",
            capture_file.display(),
            landed.display()
        );
        let script_path = write_script(dir.path(), "stub.sh", &script_contents);
        let configured = fixture_configured_script(script_path, host("m5"));

        let env = atm_core::test_support::FakeEnvSource::new([
            ("ATM_TEMP", Some("atm-temp-test-value")),
            ("ATM_IDENTITY", Some("test-agent")),
            ("ATM_TEAM", Some("test-team")),
            (
                "ATM_TRANSFER_SSH_CONFIG",
                Some("/scratch/ssh_client_config"),
            ),
        ]);

        invoke_transfer_script(
            &env,
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await
        .expect("happy path succeeds");

        let captured =
            std::fs::read_to_string(&capture_file).expect("script wrote its captured env");
        assert_eq!(
            captured.trim(),
            "ATM_TRANSFER_SSH_CONFIG=/scratch/ssh_client_config"
        );
    }

    /// ADR-055 decision (c) amendment: the child gets a synthesized,
    /// deliberately minimal `PATH` (never the caller's own). This proves
    /// both halves at once: the child's `PATH` is non-empty (the AQ4
    /// Windows regression, run 33135390308, was a completely absent
    /// `PATH`) and it is never the caller's real, distinctive `PATH`
    /// value -- forwarding that would be exactly the ambient-authority
    /// leak the rest of the allow-list already refuses.
    #[tokio::test]
    #[serial_test::serial(transfer_script_spawn)]
    async fn invoke_transfer_script_child_gets_a_synthesized_path_never_the_callers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let landed = dir.path().join("landed");
        std::fs::create_dir_all(&landed).expect("landed dir");
        let capture_file = dir.path().join("env-capture.txt");
        let script_contents = format!(
            "#!/bin/sh\nprintf '%s' \"$PATH\" > {}\necho {}\n",
            capture_file.display(),
            landed.display()
        );
        let script_path = write_script(dir.path(), "stub.sh", &script_contents);
        let configured = fixture_configured_script(script_path, host("m5"));

        let distinctive_caller_path = "/definitely-not-a-real-dir/atm-caller-path-marker";
        let env = atm_core::test_support::FakeEnvSource::new([
            ("ATM_TEMP", Some("atm-temp-test-value")),
            ("ATM_IDENTITY", Some("test-agent")),
            ("ATM_TEAM", Some("test-team")),
            ("PATH", Some(distinctive_caller_path)),
        ]);

        invoke_transfer_script(
            &env,
            &configured,
            Ulid::new(),
            &[PathBuf::from("/tmp/report.pdf")],
            Duration::from_secs(5),
        )
        .await
        .expect("happy path succeeds");

        let child_path = std::fs::read_to_string(&capture_file).expect("script captured PATH");
        assert!(!child_path.is_empty(), "child PATH must never be empty");
        assert!(
            !child_path.contains(distinctive_caller_path),
            "the caller's own PATH must never leak into the child: {child_path:?}"
        );
        assert_eq!(
            child_path,
            atm_core::transfer_script::synthesized_transfer_script_env(&env)
                .into_iter()
                .find(|(key, _)| *key == "PATH")
                .expect("synthesized env always includes PATH")
                .1
                .to_str()
                .expect("synthesized PATH is UTF-8 in this test")
        );
    }

    /// Builds a real [`ConfiguredTransferScript`] by resolving `script_path`
    /// through the production resolver against an owner-secured `transfer`
    /// directory shaped exactly like `~/.atm/transfer`, since its fields are
    /// private to `atm-core`.
    fn fixture_configured_script(script_path: PathBuf, host: HostName) -> ConfiguredTransferScript {
        let dir = script_path
            .parent()
            .expect("script has a parent")
            .to_path_buf();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
            .expect("secure transfer root");
        let final_path = dir.join(host.as_str());
        if script_path != final_path {
            std::fs::rename(&script_path, &final_path).expect("rename into host-named path");
        }
        match atm_core::transfer_script::resolve_transfer_script_in_for_tests(&dir, &host)
            .expect("resolves")
        {
            TransferScript::Configured(configured) => configured,
            TransferScript::NotConfigured { .. } => panic!("expected a configured script"),
        }
    }
}
