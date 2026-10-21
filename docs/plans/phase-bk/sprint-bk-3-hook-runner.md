# Sprint bk-3 — daemon hook runner

closure_type `boundary`; target_boundary `BOUNDARY-HookRunner`
(`DaemonHookRunner` in `atm-daemon-bootstrap` over `spawn_with_deadline`);
branch `sprint/bk-3-hook-runner`, PR → `sprint/bk-6-hook-stores-sqlite`
(layer 4). Wave 2, parallel with bk-6 and bk-7: cut from the bk-2 head,
rebased onto layer 3 in order; it consumes from bk-2 the `HookRunner` trait,
`HookTarget`, `HookRequest`, `HookReply::parse`, `StopReply::parse`,
`HookDoctorRequest`, `HookDoctorReply::parse` and from bk-1 `HookExitCause`,
the `hook_*` constructors and `HOOK_ACTION_RESERVE_MS`. Governing text:
`design.md` §3 (D-1, D-3, D-5), §9.7, §9.8, §9.10; `REQ-P-HOOK-002` §23.3
(quoted in the criteria), `REQ-P-HOOK-001`; ADR-068 D3, D4, ADR-069 D4,
ADR-066.

## Scope boundary

One crate: the process side of a hook. No filter, route, composition or
store (bk-7, bk-5). This sprint adds atm's hook subprocess call, and it
spawns only `python3 <module_path>`: atm runs no `bd` and touches no
`.beads`, so every bead call happens inside the hook process (ANC-010).
Owned paths (new unless marked edited):

- `crates/atm-daemon-bootstrap/src/process_deadline.rs`,
  `crates/atm-daemon-bootstrap/src/hook_runner.rs`,
  `crates/atm-daemon-bootstrap/src/lib.rs` (edited: `mod process_deadline;
  pub mod hook_runner;` beside `mod received_hook_selector;` `:52`; 827
  production lines, +2)
- `crates/atm-daemon-bootstrap/Cargo.toml` (edited: `async-trait.workspace`;
  `[target.'cfg(unix)'.dependencies] rustix.workspace = true`); root
  `Cargo.toml` (`rustix = { version = "1.1", features = ["process"] }`);
  `Cargo.lock` (the resulting lock lines)
- `.just/lint-config.toml` (edited: `async-trait`, `rustix` in the
  bootstrap allowlist; the `rustix` ownership entry is bk-2's)
- `crates/atm-daemon-bootstrap/tests/hook_fixture/fixture_hook.py`
  (bk-5's composition tests use it from the same crate)
- `crates/atm-architecture/tests/process_spawn_sites.rs`
- `boundaries/atm-core/hook-runner.toml` (edited: `[status]`
  `concrete_landed = ["atm_daemon_bootstrap::hook_runner::DaemonHookRunner"]`)
- `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml` (edited:
  `io_owns` + `hook_process_spawn`; `[enforcement]
  io_forbidden_source_modules = { process_spawn =
  ["atm_daemon_bootstrap::hook_runner"] }`),
  `boundaries/atm-daemon-bootstrap/daemon-observability.toml` (both:
  `async-trait`, `rustix` in `allowed_dependencies`)

bk-2 lands the `rustix` and `cron` ownership entries and the `atm-cron`
section rule. bk-4 must_follow this sprint and starts from its final head,
building on these root `Cargo.toml`, `Cargo.lock` and
`.just/lint-config.toml` edits (Rand, 2026-10-19: "do 4 waves").

## Public contract

```rust
// crates/atm-daemon-bootstrap/src/process_deadline.rs   (design.md §9.7)
pub(crate) struct SpawnSpec { pub program: PathBuf, pub args: Vec<OsString>, pub env: Vec<(OsString, OsString)>, pub cwd: PathBuf }
pub(crate) struct SpawnedOutput { pub stdout: Vec<u8>, pub stderr: Vec<u8>, pub status: std::process::ExitStatus, pub elapsed_ms: u64 }
pub(crate) enum SpawnFailure { TimedOut { after_ms: u64 }, Failed(AtmError) }   // DaemonHookRunner adds the phase
pub(crate) const STDOUT_CAP: u64 = 1 << 20;  pub(crate) const STDERR_CAP: u64 = 64 << 10;
/// Spawns (env_clear, envs, current_dir, piped stdio, kill_on_drop, unix process_group(0)) and races `cancel` against one future joining
/// the stdin write-then-close (BrokenPipe ignored), the wait and both bounded take() reads. `cancel` first: group kill, `kill().await`
/// (reaped), then TimedOut. Over a cap: group kill, reap, Failed(hook_malformed). Spawn error: Failed(hook_exit(Spawn{..})). Never retries.
pub(crate) async fn spawn_until(spec: SpawnSpec, stdin: &[u8], cancel: impl Future<Output = ()>) -> Result<SpawnedOutput, SpawnFailure>;
/// The production entry: spawn_until(spec, stdin, tokio::time::sleep(remaining)), after_ms = remaining.
pub(crate) async fn spawn_with_deadline(spec: SpawnSpec, stdin: &[u8], remaining: Duration) -> Result<SpawnedOutput, SpawnFailure>;
/// Held from spawn until the child is reaped; Drop (task aborted, future dropped) sends SIGKILL to the group, so an abort orphans no
/// grandchild. unix: rustix::process::kill_process_group(pid, Signal::KILL); windows: kill_on_drop alone (the hook process only, ADR-068 D4).
struct GroupKillGuard { pgid: Option<rustix::process::Pid>, armed: bool }

// crates/atm-daemon-bootstrap/src/hook_runner.rs
pub const HOOK_CONCURRENCY_MAX: usize = 16;   // live hook children per daemon: task chains, cron runs and doctor share it
pub struct DaemonHookRunner { base_env: Result<Vec<(OsString, OsString)>, AtmError>, python: Result<PathBuf, AtmError>, permits: Arc<tokio::sync::Semaphore> }
impl DaemonHookRunner {
    pub fn new(env: &dyn EnvSource) -> Self;                                  // with_limit(env, HOOK_CONCURRENCY_MAX); never fails, errors returned per call
    pub(crate) fn with_limit(env: &dyn EnvSource, permits: usize) -> Self;
}
/// Pure (D-1): ATM_TEAM, ATM_PHASE, PATH, HOME (unix: PATH, HOME required); windows: HOME optional, SystemRoot and USERPROFILE required,
/// PATH found case-insensitively; python resolved on that PATH with PATHEXT. A missing required key is hook_exit(Missing(<key>)).
pub(crate) fn hook_env(env: &dyn EnvSource, team: &TeamName, phase: &PhaseId, windows: bool) -> Result<Vec<(OsString, OsString)>, AtmError>;
impl atm_core::boundary::sealed::Sealed for DaemonHookRunner {}
#[async_trait] impl HookRunner for DaemonHookRunner {}
// argv [module_path, request.hook] for run and stop, [module_path, "doctor"] for doctor. Budget first: run, stop use runner_budget, None is
// hook_timeout(phase, "request budget exhausted before the hook ran"); doctor uses doctor_budget, None is hook_timeout(phase, "doctor budget
// exhausted"); no spawn either way. Then timeout(budget, permits.acquire_owned()): elapsed is hook_timeout(phase, "hook concurrency limit
// reached"), no spawn; the permit is held until the child is reaped. After `spec()` validates the target, an exhausted
// `budget - waited` is hook_timeout(phase, "budget exhausted before spawn"), no spawn; otherwise spawn_with_deadline uses it:
// TimedOut -> hook_timeout(phase, "no reply within <n> ms (limited by configured timeout|request deadline)"); exit n -> hook_exit(Exit{code: n, stderr_tail}); signal ->
// hook_exit(Signal(n)); exit 0 -> HookReply::parse / StopReply::parse / HookDoctorReply::parse of stdout, errors unchanged.
/// The only task-hook and cron budget arithmetic: Some(min(timeout_ms, remaining - HOOK_ACTION_RESERVE_MS)) when positive, else None.
pub(crate) fn runner_budget(timeout_ms: u32, remaining: Duration) -> Option<Duration>;
/// Doctor, no reserve: Some(min(timeout_ms, remaining)) when remaining > 0, else None.
pub(crate) fn doctor_budget(timeout_ms: u32, remaining: Duration) -> Option<Duration>;
```

Failure table (`design.md` §9.7, messages from bk-1's §9.1 constructors):
child exits n ≠ 0 → `ATM_HOOK_EXIT` `exit n: <stderr tail>`; killed by
signal → `ATM_HOOK_EXIT` `terminated by signal n`; Python exception →
`ATM_HOOK_EXIT` with the traceback tail; `python3` not on `PATH` →
`ATM_HOOK_EXIT` `python3 not found on PATH`; module or cwd missing →
`ATM_HOOK_EXIT` `cannot run <path>: <cause>`; no reply within budget →
`ATM_HOOK_TIMEOUT`; stdout over 1 MiB or not one of the four shapes →
`ATM_HOOK_MALFORMED`. stderr is read only for the tail (64 KiB cap).

Why a new helper rather than reuse (one-line record, design §9.7):
`received_hook_selector.rs:467` and `atm-herdr/src/transport_cli.rs:121`
kill only the leader, write no stdin and read unbounded output; a hook
needs a group kill, a stdin body and capped reads, so those two stay
unchanged and hooks get `spawn_until`.

## Authoritative deliverables

1. `spawn_until`, `spawn_with_deadline`, `GroupKillGuard` and
   `DaemonHookRunner` as above; `hook_runner.rs` spawns only through the
   helper.
2. `process_spawn_sites.rs`: a `syn` visitor over the non-test source of
   every `crates/*/src` (items under `#[cfg(test)]` and `tests/` dirs
   skipped). A site is a `Command::new` call in a file that imports
   `tokio::process::Command` or `std::process::Command` (or an alias of
   either), or a fully qualified `{tokio,std}::process::Command::new`. The
   recorded set: tokio — `atm-daemon-bootstrap/src/process_deadline.rs`,
   `atm-daemon-bootstrap/src/received_hook_selector.rs`,
   `atm-herdr/src/transport_cli.rs`, `atm/src/commands/send_to.rs`; std —
   `atm-daemon-client/src/lib.rs`, `atm/src/commands/internal_nudge.rs`,
   `sc-lint-boundary/src/graph.rs`. REQ-BK-D3's locked check names this
   file; `io_forbidden_source_modules` covers one crate's modules, this
   covers the workspace.
3. `fixture_hook.py` (§9.8 `__main__` block): prints `fixture_reply.json`
   from its cwd, writes stdin to `fixture_request.json` and `os.environ`
   to `fixture_env.json`; implements `task.close` unless
   `fixture_hooks.txt` lists others.
4. Tests, deterministic, no daemon (REQ-BK-TESTS). A real child runs on the
   real clock and asserts content, never timing: a reply path uses
   `timeout_ms = 30000`, `RequestDeadline::after(60 s)`; a timeout case uses
   a child sleeping 3600 s, so the deadline always fires first. Kills are
   observed through a FIFO (made by a synchronous `python3 -c
   "import os,sys; os.mkfifo(sys.argv[1])"`): the module opens its write
   end, forks a grandchild that inherits it, writes `r`, and both sleep
   3600 s; the test reads `r` from a blocking reader, cancels, and
   `read_to_end` returning EOF proves both writers died. No pid probes, no
   sleeps. Budget and permit paths with no child run under `start_paused`.

## Authoritative acceptance criteria

`crates/atm-daemon-bootstrap/src/hook_runner.rs::tests` unless stated:

- [ ] (ANC-012) `boundary:BOUNDARY-HookRunner` `process_deadline.rs::tests::cancel_kills_child_and_grandchild` (`#[cfg(unix)]`, FIFO above; the cancel future is a oneshot fired after `r`): `Err(TimedOut)` is returned after the child is reaped and the FIFO reaches EOF; `::abort_kills_the_process_group` (the same, aborting the task running `spawn_until` instead: EOF); `runner_timeout_returns_hook_timeout` (module that never replies, `timeout_ms = 200`: `ATM_HOOK_TIMEOUT`); `timeout_reports_effective_budget_and_binding_limit` (controlled inputs pin exact messages: `no reply within 200 ms (limited by configured timeout)` and `no reply within 75 ms (limited by request deadline)`).
- [ ] (REQ-BK-ERRORS) `boundary:BOUNDARY-HookRunner` each message pinned to the failure table: `nonzero_exit_returns_hook_exit_with_stderr_tail` (`HOOK.EXIT: exit 3: <tail>`), `hook_killed_by_signal_reports_signal` (unix, the module kills itself with SIGKILL: `terminated by signal 9`), `module_exception_returns_hook_exit_with_traceback_tail` (holds `ValueError: boom`), `stdout_over_cap_is_malformed_and_child_reaped`, `not_json_stdout_is_malformed`, `python3_missing_returns_hook_exit` (empty `PATH` dir), `{module_path,cwd}_missing_returns_hook_exit` (`cannot run <path>: `), `zero_remaining_budget_returns_timeout_without_spawn` (`RequestDeadline::after(ZERO)`: `request budget exhausted before the hook ran`, no `fixture_request.json`), `doctor_expired_deadline_times_out_without_spawn`.
- [ ] (service limit) `boundary:BOUNDARY-HookRunner` `saturated_runner_times_out_without_spawn` (`start_paused`, `with_limit(env, 0)`, `timeout_ms = 2500`, deadline 10 s: `HOOK.TIMEOUT: phase bk: hook concurrency limit reached`, no `fixture_request.json`).
- [ ] (§9.7 stdin) `boundary:BOUNDARY-HookRunner` `module_exiting_without_reading_stdin_reports_its_exit_code` (128 KiB `custom`, `sys.exit(3)` first: `HOOK.EXIT: exit 3`); `module_never_reading_stdin_times_out` (128 KiB, sleeps 3600 s, `timeout_ms = 200`: `ATM_HOOK_TIMEOUT`).
- [ ] (D-5) `boundary:BOUNDARY-HookRunner` `runner_budget_rules` (pure): `(2500, 10 s)` → 2500 ms, `(2500, 2 s)` → 1500 ms, `(30000, 600 ms)` → 100 ms, `(2500, 500 ms)` and `(2500, 0)` → `None`; `doctor_budget_has_no_reserve`: `(2500, 600 ms)` → 600 ms, `(2500, 10 s)` → 2500 ms, `(2500, 0)` → `None`.
- [ ] (ANC-003, ANC-022) `boundary:BOUNDARY-HookRunner` `stdin_is_the_request_and_custom_is_byte_identical`: `fixture_request.json` equals `serde_json::to_vec(&request)` and its `custom` equals `serde_json::to_string(&target.custom)` for `{"goal":"ship","n":[1,{"y":null}]}`; argv[1] is `request.hook`.
- [ ] (REQ-BK-D1) `boundary:BOUNDARY-HookRunner` `runner_passes_daemon_path_and_home_values`: `fixture_env.json` keys are exactly `{ATM_TEAM, ATM_PHASE, PATH, HOME}` on unix with the values given to `new`, and `os.getcwd()` is the target cwd; `hook_env_rules` (pure, both `windows` values): unix without `HOME` or `PATH` is `hook_exit(Missing)`; windows without `HOME` succeeds, without `SystemRoot` or `USERPROFILE` fails, and `Path` is found as `PATH`; `#[cfg(windows)] windows_runner_works_without_home_and_with_path_cased_Path`.
- [ ] (ADR-068 D3, ADR-069 D4, ANC-024) `boundary:BOUNDARY-HookRunner` `doctor_sends_team_phase_custom_and_returns_findings` (argv[1] `doctor` for a registration and a schedule target, stdin `HookDoctorRequest::for_target`; the §9.10 example gives one finding; `{"ok":1}` is `ATM_HOOK_MALFORMED`); `stop_parses_both_shapes_and_refuses_missing_reason` (argv[1] and stdin `hook` are `cron.stop`).
- [ ] (REQ-BK-D3) `boundary:BOUNDARY-HookRunner` `process_spawn_sites.rs::process_spawn_sites_are_the_recorded_files` (the seven files of deliverable 2, paths relative to `crates/`, split by `tokio`/`std`); `received_hook_selector.rs` is unchanged; `just lint` passes with `process_spawn` forbidden in `hook_runner` and `hook-runner.toml` `concrete_landed`.
- [ ] (ANC-010) `boundary:BOUNDARY-HookRunner` `rg -n '"bd"|\.beads' crates/atm-daemon-bootstrap/src` matches nothing.

## Authoritative validation

- `cargo test -p atm-daemon-bootstrap -p atm-architecture --test process_spawn_sites`
- `cargo clippy -p atm-daemon-bootstrap --all-targets -- -D warnings`
- `just lint` (cargo-shear, boundaries, `.just/check_line_counts.py`)

## Non-closure

- No filter, dispatcher or route (bk-7); no composition, chain tracker
  wiring or doctor collection (bk-5); no cron tick (bk-4).
