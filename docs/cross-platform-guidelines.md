# Cross-Platform Guidelines

Rules and patterns for ensuring atm works correctly on Ubuntu, macOS, and Windows CI.

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

## Clippy Compliance

CI runs Rust 1.93 clippy with `-D warnings`. Local toolchains may be older and miss lints.

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
