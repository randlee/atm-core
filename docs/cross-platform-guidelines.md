# Cross-Platform Guidelines

Rules and patterns for ensuring atm works correctly on Ubuntu, macOS, and Windows CI.

## Windows Benchmark Execution

Use the dedicated benchmark OS account and the canonical
[`benchmark-run` skill](../.claude/skills/benchmark-run/SKILL.md). Windows runs
the `sqlite`, `tcp`, and `tcp-tls` matrix; Unix-domain sockets are not a
Windows target. Before a run, set
`ATM_CAPACITY_HOST_LABEL=windows-x64-01` and select the paired CLI and daemon
through daemon-switch's `atm.exe` and `atm-daemon.exe` selector symlinks. Run
the native PowerShell command `just benchmark`, then `just benchmark-show`,
then `just benchmark-publish`; do not use WSL or replace an installed binary.

## Phase S Daemon Portability Guard

For same-host daemon functionality, operating-system-specific implementation
differences are allowed only in these daemon-owned areas:
- local IPC transport adapter
- lifecycle-control source adapter
- host-ownership adapter

Do not solve Windows support by scattering `#[cfg(unix)]` / `#[cfg(windows)]`
branches through runtime composition, dispatcher, replay, status-cache,
watch/reconcile, or notifier code.

Review rule:
- if a new same-host daemon feature cannot be implemented on every supported
  operating system through the documented portability boundaries, the docs and
  architecture must be updated before implementation continues

## No-Flaky-Test Policy

Cross-platform daemon and runtime tests must not rely on timing luck and must
not contain a path that can block indefinitely.

Required shapes:
- bounded channel handshakes
- `Barrier`, `Condvar`, or predicate synchronization with a bounded wait
- bounded readiness and shutdown probes tied to observable state
- panic-safe cleanup for shared/global test hooks

Forbidden shapes:
- fixed sleeps as the primary correctness mechanism
- retry-until-success loops with no predicate or deadline
- unbounded `recv()`, `wait()`, or equivalent blocking calls in risky
  same-host daemon/runtime tests
- bare `join()` when the test has no bounded proof of completion first

Mechanical-enforcement rule:
- if a prohibited pattern is cheap and deterministic to detect, it belongs in
  `just lint` rather than review-only guidance

## Windows Same-Host Runtime Contracts

### Host-Ownership Sidecar Recovery

Windows host-ownership recovery uses a same-directory sidecar shadow file:
- lock file: `owner.lock`
- shadow file: `owner.lock.meta`

Required behavior:
- write the canonical `pid:token` owner record to the locked file
- mirror the same record to `owner.lock.meta` with temp-file-plus-rename replacement
- when reading the locked file fails with a Windows lock/sharing violation during stale-owner
  recovery, fall back to `owner.lock.meta`
- reject recovery if the pid/token changes between the first stale-owner observation and the
  eventual retry lock acquisition

This sidecar is a recovery aid only. It does not replace the locked file as the source of truth
while the lock handle remains readable.

### `ErrorKind::Unsupported` Is Not a Timeout Exemption

The current legacy Windows local transport reports `ErrorKind::Unsupported` for `set_recv_timeout()` and
`set_send_timeout()`. Code may tolerate that result only when it immediately installs a bounded
fallback contract.

Required fallback shapes:
- request reads: a watchdog or equivalent bounded fallback that prevents blocked Windows
  local-transport reads from pinning the connection slot or shutdown drain past the documented deadline
- shutdown wake connections: no unbounded `flush()`/drain after the `Unsupported` bypass; the wake
  path must still return within the same bounded deadline
- shutdown drain: unsupported socket deadlines must not leave `active_connections` pinned past the
  forced-cancel window

Forbidden shape:
- swallowing `ErrorKind::Unsupported` and then proceeding to an unbounded read, write, flush, or
  shutdown wait

### AD.30 Windows Local IPC Depth Cases

The accepted Windows daemon depth closure covers exactly these same-host local
IPC cases:
- dispatcher panic during shutdown
- injected accept-error handling
- post-terminate connection rejection

Required proof shape:
- the tests must run through the accepted runtime/local-IPC hooks rather than
  a Windows-only alternate contract
- dispatcher panic and injected accept-error paths must each produce one
  retained observable failure record plus bounded completion
- injected accept-error handling is a fail-fast typed-error path on the
  accepted line; it is not a retry/backoff scenario
- post-terminate rejection must fail quickly after terminate and must not rely
  on a late connection eventually timing out

## Windows Smoke Test

Use this procedure on a fresh Windows checkout of `feature/windows-test-parity`. The Windows
machine should treat Git as the handoff channel: pull this branch first, then run the steps below
from Windows PowerShell in the repository root.

Observed result on the first Windows machine for this branch:
- `PASS` after pulling `feature/windows-test-parity` and running the same-host daemon smoke flow
- verified outcomes: `doctor --json` reached ready, the local endpoint was published, `send` succeeded,
  and `read --all --json` returned the delivered body
- one environment caveat: stale local daemon/test processes can hold the host owner lock and must
  be cleared before rerunning the smoke

1. Prerequisites
   - Install the Rust MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc` or equivalent).
   - Clone `atm-core`, then pull the branch under test:
   ```powershell
   git fetch origin
   git switch feature/windows-test-parity
   git pull --ff-only origin feature/windows-test-parity
   ```

2. Create a disposable ATM environment
   ```powershell
   $SmokeRoot = Join-Path $env:TEMP "atm-win-smoke"
   Remove-Item $SmokeRoot -Recurse -Force -ErrorAction SilentlyContinue
   New-Item -ItemType Directory -Force -Path $SmokeRoot | Out-Null
   Set-Content -Path (Join-Path (Get-Location) ".atm.toml") -Value "[atm]`ndefault_team = `"smoke-team`"`n"
   $env:ATM_HOME = $SmokeRoot
   $env:ATM_CONFIG_HOME = $SmokeRoot
   $env:ATM_TEAM = "smoke-team"
   $env:ATM_IDENTITY = "smoke-user"
   ```
   Windows local clients discover the daemon-owned `local-http.json` record
   under the runtime directory. Do not set a local socket or fixed-port
   environment variable.
   Then initialize the roster with ATM itself:
   ```powershell
   .\target\debug\atm.exe teams add-member smoke-team smoke-user worker gpt-5 --home-dir $SmokeRoot
   ```

3. Build the workspace
   ```powershell
   just build
   ```
   Pass indicator:
   - workspace build exits zero
   - `target\debug\atm-daemon.exe` and `target\debug\atm.exe` exist
   Fail indicator:
   - `just build` exits non-zero or the binaries are missing

4. Run `atm doctor` and confirm the daemon reaches ready state
   ```powershell
   $Doctor = .\target\debug\atm.exe doctor --json | ConvertFrom-Json
   $Doctor.summary.status
   $Doctor.runtime_status.readiness
   ```
   Pass indicator:
   - `summary.status` is `healthy` or `warning`
   - `runtime_status.readiness` is `ready`
   Fail indicator:
   - `doctor` exits non-zero
   - `runtime_status.readiness` is absent or not `ready`

5. Verify the current local endpoint remains usable
   ```powershell
   .\target\debug\atm.exe doctor --json
   ```
   Pass indicator:
   - `runtime_status.readiness` remains `ready` after the client reaches the daemon
   Fail indicator:
   - the command reports daemon unavailable or a non-ready runtime

6. Confirm the daemon accepts a connection and a round-trip mailbox operation
   ```powershell
   .\target\debug\atm.exe send smoke-user "windows smoke hello" --json
   .\target\debug\atm.exe read --all --json
   ```
   Pass indicator:
   - `send` returns a normal sent result
   - `read --all --json` includes the `windows smoke hello` body for `smoke-user@smoke-team`
   Fail indicator:
   - `send` reports daemon unavailable / connection refused
   - `read` does not show the just-sent message

## Home Directory Resolution

**Problem**: `dirs::home_dir()` on Windows uses the Windows API (`SHGetKnownFolderPath`), which ignores both `HOME` and `USERPROFILE` environment variables. Tests that only redirect `HOME` do not relocate the canonical `~/.claude` config root on Windows.

**Solution**:
- `ATM_HOME` controls the runtime root for sockets, logs, and other daemon state.
- `ATM_CONFIG_HOME` controls the canonical config root used by `get_os_home_dir()`.
- Tests may still set `HOME` for Unix parity, but correctness must not depend on it.

```rust
pub fn get_os_home_dir() -> Result<PathBuf> {
    if let Ok(home) = std::env::var("ATM_CONFIG_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine OS home directory"))
}
```

### Integration Test Pattern (MANDATORY)

Every integration test file MUST use this helper:

```rust
fn set_home_env(cmd: &mut assert_cmd::Command, temp_dir: &TempDir) {
    let runtime_home = temp_dir.path().join("runtime-home");
    cmd.env("ATM_HOME", &runtime_home)
        .env("ATM_CONFIG_HOME", temp_dir.path());
}
```

`ATM_CONFIG_HOME` is the required cross-platform override for config-root isolation. Setting `HOME` alone is never sufficient on Windows.

### Verification

Before declaring dev work complete, grep all integration test files:
```bash
grep -rn 'ATM_CONFIG_HOME' crates/atm/tests/ || echo "FAIL: Missing ATM_CONFIG_HOME in test helpers"
grep -rn 'env(\"HOME\"' crates/atm/tests/
```

### Planned Phase S.5 Guardrails

Immediate/default-lint families:
- fixed-sleep test hygiene, with the current repository-local rule treated as
  the proving implementation for later `sc-lint` extraction
- daemon-spawn and warmup-helper rejection
- production bare `Condvar::wait(...)`
- production discarded `wait_timeout*` results
- targeted same-host daemon test checks for cheap unbounded-wait syntax once
  the repository-local rule is landed

Deferred analyzer families:
- path-sensitive `JoinHandle::join()` safety
- polling-loop terminate-state placement
- panic-safe cleanup proof for shared/global hook registries
- bounded-wait result handling in test code

## Clippy Compliance

CI runs Rust 1.94.1 clippy with `-D warnings`. Local toolchains may be older and miss lints.

### Known Strict Lints

- **`collapsible_if`**: Nested `if`/`if let` chains must be collapsed using let chain syntax (stable since Rust 1.87):
  ```rust
  // BAD: nested if
  if path.is_file() {
      if let Ok(content) = fs::read_to_string(&path) {
          // ...
      }
  }

  // GOOD: collapsed with let chain
  if path.is_file()
      && let Ok(content) = fs::read_to_string(&path)
  {
      // ...
  }
  ```

- **Deprecated APIs**: Use `assert_cmd::cargo::cargo_bin_cmd!("atm")` instead of the deprecated `Command::cargo_bin("atm")`.

### Pre-Commit Check

Always run before declaring implementation complete:
```bash
cargo clippy -- -D warnings
```

## Temporary Files and Directories

**Problem**: `/tmp/` is a Unix-only path. Windows has no `/tmp/` directory — hardcoding it causes immediate failure on Windows CI.

**Solution**: Use `std::env::temp_dir()` for any temporary file path in production code. Use `tempfile::TempDir` for test isolation.

```rust
// BAD: Unix-only, fails on Windows
let path = PathBuf::from("/tmp/atm-session-id");

// GOOD: cross-platform
let path = std::env::temp_dir().join("atm-session-id");
```

**In tests**, always use a scoped `TempDir` rather than a fixed temp path — this avoids both the `/tmp` problem and test interference:

```rust
// BAD: hardcoded /tmp path in test
let path = PathBuf::from("/tmp/test-artifact");

// GOOD: temp_env-isolated TempDir
let dir = tempfile::tempdir().expect("temp dir");
let path = dir.path().join("test-artifact");
```

### Verification

Before declaring dev work complete, grep for hardcoded `/tmp`:
```bash
grep -rn '"/tmp/' crates/ && echo "FAIL: Found /tmp hardcoding" || echo "OK"
grep -rn "'/tmp/" crates/ && echo "FAIL: Found /tmp hardcoding" || echo "OK"
```

## File Paths

- Use `std::path::Path` and `PathBuf` for all file operations (not string concatenation).
- Use `path.join()` for path construction (handles separators cross-platform).
- Never hardcode `/` or `\` as path separators.

## Environment Variables

- Check env vars with `std::env::var()`, not by reading `/proc` or shell config files.
- For test isolation, set env vars per-command with `cmd.env("KEY", "value")` rather than `std::env::set_var()` which is global and causes race conditions in parallel tests.

## Test Subprocess Isolation

See also:
- [`requirements.md`](./requirements.md) `REQ-CORE-TEST-001`
- [`.claude/agents/arch-qa.md`](../.claude/agents/arch-qa.md) `RULE-011`

Subprocess-style ATM tests must not reuse developer workstation config or
identity state.

Required pattern:
- create one `TempDir` per test fixture
- point `ATM_HOME` at that temp-owned runtime root
- point `ATM_CONFIG_HOME` at that temp-owned config root
- point `ATM_TEAMS_DIR` at a temp-owned teams directory when the test depends
  on team discovery
- pass environment overrides with `cmd.env(...)` on each spawned command

Use explicit test-only names for team and agent fixtures:
- `TEST_TEAM = "test-team"`
- `TEST_SENDER = "test-sender"`
- `TEST_RECIPIENT = "test-recipient"`
- `TEST_LEAD = "test-lead"`

Production role semantics may still matter in a few tests:
- `ATM_TEAM` and `ATM_IDENTITY` may be set explicitly when the test is proving
  production env-read behavior
- `team-lead` may be used when the role itself is semantically significant,
  but the raw literal should be centralized behind one constant such as
  `ROLE_TEAM_LEAD`

Avoid:
- raw `atm-dev` / `arch-*` literals in generic test fixtures
- `std::env::set_var()` in integration tests
- reading or writing the developer's real ATM home during tests

Recommended helper shape:

```rust
let env = TestEnvBuilder::new().build().expect("test env");
let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("atm");
// TestEnvBuilder provides filesystem isolation only. Tests still set
// ATM_IDENTITY and ATM_TEAM explicitly when the command under test needs them.
cmd.envs(env.env_map.iter())
    .env("ATM_IDENTITY", TEST_SENDER)
    .env("ATM_TEAM", TEST_TEAM)
    .current_dir(&env.cwd);
```

## Line Endings

- Rust's `fs::read_to_string()` returns platform-native line endings.
- When comparing file content in tests, avoid hardcoding `\n`. Use `.contains()` or `.lines()` for line-by-line comparison.
- The `.gitattributes` file should enforce consistent line endings for source files.

## Lifecycle Transition Event Scope

- Lifecycle transition events that rely on PID liveness transitions (`member_state_change`,
  `member_activity_change`, `session_id_change`, `process_id_change`) are currently
  **Unix-only** because PID existence probing is Unix-specific in this code path.
- Implement these assertions and CI expectations behind `#[cfg(unix)]` until a
  Windows-equivalent PID validation backend is added.
