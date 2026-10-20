# Sprint bk-5 — hook and cron CLI, doctor hooks section, composition, user docs

closure_type `integration`; target_boundary the `atm` CLI (`atm hook`,
`atm cron`, the `atm doctor` hooks lines), the daemon composition of the
filter, `atm-cron` and the shutdown order, over `req:REQ-P-HOOK-001`,
`req:REQ-P-HOOK-003`, `req:REQ-P-HOOK-004`, `req:REQ-P-CRON-001`,
`req:REQ-P-CRON-002`; branch `sprint/bk-5-hook-cli-doctor`, PR →
`sprint/bk-4-atm-cron` (layer 7, wave 4). Consumes bk-6
(`StorageHandles::hook_registration_store`, `cron_schedule_store`), bk-3
(`DaemonHookRunner`, `fixture_hook.py`), bk-7 (routes, `X-ATM-Hook-Actions`,
`NormalTaskCommands`, `HookedTaskCommands`, `ActiveHookSnapshot`,
`ChainTracker`, `unhooked()`, `install_cron_scheduler`, `DoctorReport.hooks`)
and bk-4 (`AtmCron`, `AtmCronDeps`, `CronWorkerHandle`, the clocks and
jitters). Governing text: `design.md`
§2.3, §5, §6, §9.1, §9.8, §9.9, D-2, D-5, D-9; `REQ-P-HOOK-003` §23.1,
`REQ-P-HOOK-004` §11.5, `REQ-P-CRON-001` §24.1, `REQ-P-CRON-002` §24.2,
`REQ-P-USER-DOCS-001`, `REQ-P-USER-DOCS-002`; ADR-068 D3, D5, D6, ADR-069
D1, D2, ADR-066.

## Scope boundary

No storage, protocol, runner, filter, route or scheduler code;
`crates/atm/Cargo.toml` unchanged. Owned paths (new unless marked edited;
production line counts):

- `crates/atm/src/commands/hook.rs`, `crates/atm/src/commands/cron.rs`,
  `crates/atm/src/commands/hook_module.rs`, `crates/atm/src/commands/mod.rs`
  (edited: `Hook(HookCommand)`, `Cron(CronCommand)` in `enum Command` at
  `:118` and their arms; 130), `crates/atm/src/output/hooks.rs`
  (`print_hook_list`, `print_cron_list`, `print_doctor_hooks`,
  `print_hook_actions`), `crates/atm/src/output.rs` (edited: `mod hooks;
  pub use hooks::*;` and one `print_doctor_hooks` call in
  `print_doctor_result` `:288`; 991 → 994), `crates/atm/src/composition.rs`
  (edited: the `HookActions` arm; 411), `crates/atm/src/main.rs` (edited:
  three codes in the `=> 3` arm of `exit_code_for_atm_error` `:138-151`;
  789)
- `crates/atm/tests/cli_surface_baseline.json` (edited), data read via
  `CARGO_MANIFEST_DIR`, no new `[[test]]`, under
  `crates/atm/tests/fixtures/hook_module/`: `valid_hook.py`,
  `no_timeout.py`, `zero_timeout.py`, `over_cap.py`, `indented.py`,
  `two_lines.py`, `cron_timeout.py` (`CRON_TIMEOUT_MS = 20000`),
  `cron_over_cap.py` (`CRON_TIMEOUT_MS = 30001`)
- `crates/atm-runtime/src/composition.rs` (edited:
  `.with_hook_registration_store(storage.hook_registration_store())` and
  `.with_cron_schedule_store(..)` beside `.with_task_store` `:193`; 276)
- `crates/atm-daemon-bootstrap/src/replacement_handler.rs` (edited:
  `compose_storage_router` `:268`, `build_replacement_handler` `:204`
  returning the four-tuple; 340), `crates/atm-daemon-bootstrap/src/lib.rs`
  (edited: the call sites `:471`, `:1303`, `:1505`, `mod
  cron_composition;`, `DaemonWorkers.cron` `:532`, calls from
  `run_replacement_daemon_with_selector` `:445` and
  `shutdown_replacement_daemon` `:805`; 827 → about 840),
  `crates/atm-daemon-bootstrap/src/telemetry_lifecycle_tests.rs` (edited:
  its call `:172`), `crates/atm-daemon-bootstrap/src/cron_composition.rs`,
  `crates/atm-daemon-bootstrap/Cargo.toml` (edited: `atm-cron.workspace`;
  dev `atm-cron` with `test-utils`), `Cargo.lock`,
  `crates/atm-daemon-bootstrap/src/hook_cron_composition_tests.rs`,
  `crates/atm-daemon-bootstrap/tests/cron_fixture/fixture_cron.py`
- `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`,
  `boundaries/atm-daemon-bootstrap/daemon-observability.toml` (edited:
  `atm-cron` in `allowed_dependencies`; `cron_worker_lifecycle` in the
  former's `io_owns`), `boundaries/atm-cron/cron-scheduler.toml` (edited:
  `[status]`), `.just/lint-config.toml` (edited: `atm-cron` in the
  bootstrap allowlist)
- `docs/user-documents/cli-reference.md` (edited: `` ### `atm cron` `` and
  `` ### `atm hook` `` in alphabetical position, a hooks paragraph under
  `` ### `atm doctor` `` `:109`), `docs/user-documents/tasks.md` (edited:
  "Task hooks"), `docs/user-documents/hooks.md` (edited: today's post-send
  hook sections stay under "Post-send hooks", still linked from
  `README.md:26`; the task-hook sections below are added and
  `reviewed_for_release` set to the phase's release version)

## Public contract

```rust
// crates/atm/src/commands/hook.rs  (clap, pattern of task.rs:41)
#[derive(Debug, Parser)] pub struct HookCommand { #[command(subcommand)] pub command: HookSubcommand }
#[derive(Debug, Subcommand)] pub enum HookSubcommand { Register(HookRegisterArgs), Disable(HookDisableArgs), List(HookListArgs) }
#[derive(Debug, Args)] pub struct HookRegisterArgs { pub hook: String, #[arg(long)] pub module: PathBuf, #[arg(long)] pub phase: String, #[arg(long)] pub cwd: Option<PathBuf>,
    #[arg(long, value_parser = parse_custom_object)] pub custom: Option<serde_json::Map<String, serde_json::Value>> }   // absent sends {}
#[derive(Debug, Args)] pub struct HookDisableArgs { #[arg(long)] pub phase: String, pub hook: Option<String> }
#[derive(Debug, Args)] pub struct HookListArgs { #[arg(long)] pub phase: Option<String>, #[arg(long)] pub json: bool }

// crates/atm/src/commands/cron.rs
#[derive(Debug, Subcommand)] pub enum CronSubcommand { Start(CronStartArgs), End(CronEndArgs), List(CronListArgs) }
#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("stop").required(true).multiple(false).args(["until", "until_hook"]))]
pub struct CronStartArgs { pub name: String, #[arg(long = "cron")] pub cron_expr: String, #[arg(long)] pub module: PathBuf, #[arg(long)] pub phase: String,
    #[arg(long, value_parser = parse_rfc3339_utc)] pub until: Option<DateTime<Utc>>, #[arg(long = "until-hook")] pub until_hook: bool,
    #[arg(long = "arg", action = clap::ArgAction::Append, allow_hyphen_values = true)] pub args: Vec<String>,   // design D-j: --arg status --arg --json
    #[arg(long)] pub cwd: Option<PathBuf>, #[arg(long, value_parser = parse_custom_object)] pub custom: Option<serde_json::Map<String, serde_json::Value>> }
#[derive(Debug, Args)] pub struct CronEndArgs { #[arg(long)] pub phase: String, pub name: Option<String> }
#[derive(Debug, Args)] pub struct CronListArgs { #[arg(long)] pub phase: Option<String>, #[arg(long)] pub json: bool }

// crates/atm/src/commands/hook_module.rs  (design.md §9.8 grammar, std only; no cap check: the daemon routes own the caps, design §9.7)
pub fn read_timeout_ms(module: &Path) -> Result<u32, AtmError>;   // Err is AtmError::hook_module_invalid naming the module and the rule broken
/// CRON_TIMEOUT_MS by the same grammar; absent: read_timeout_ms (Rand 2026-10-18).
pub fn read_cron_timeout_ms(module: &Path) -> Result<u32, AtmError>;
pub fn parse_custom_object(raw: &str) -> Result<serde_json::Map<String, serde_json::Value>, String>;   // "--custom must be a JSON object: <serde error>"
pub fn parse_rfc3339_utc(raw: &str) -> Result<DateTime<Utc>, String>;   // clap value_parser for --until; "--until must be RFC 3339: <chrono error>"

// crates/atm/src/output/hooks.rs
pub fn print_hook_list(rows: &[HookRegistration], json: bool) -> Result<()>;   // PHASE HOOK ACTIVE MODULE CWD TIMEOUT_MS CUSTOM REGISTERED_BY REGISTERED_AT DISABLED_AT
pub fn print_cron_list(rows: &[CronSchedule], json: bool) -> Result<()>;       // PHASE NAME ACTIVE CRON JITTER_S MODULE ARGS TIMEOUT_MS CUSTOM STOP LAST_RUN_AT LAST_OUTCOME
// CUSTOM prints `<n> keys`; --json carries the full custom object and jitter_s
pub fn print_doctor_hooks(entries: &[HookDoctorEntry]) -> Vec<String>;   // `Hooks:` + one line per entry + indented findings, or `Hooks: none`
pub fn print_hook_actions(response: &HookActionsResponse) -> String;     // `hook ran <n> actions` (D-b)
```

Each command does these steps:

- **Preflight.** `preflight_daemon_api(.., "1.13.0", ..)` (`send.rs:717`);
  team from caller context (ADR-066).
- **`hook register`.** `read_timeout_ms` (grammar only; the route caps).
  `--cwd` defaults to the module's parent; both canonicalized; a missing
  one exits 3 with `ATM_HOOK_MODULE_INVALID` before any request (also
  `cron start`). Sends `HookRegister` (`custom` from `--custom`), prints
  `registered <hook> for <team>/<phase> (timeout <n> ms)`. The hook name
  is unchecked; the daemon's error prints unchanged.
- **`hook disable`, `cron end`.** One line per row changed; with none,
  `no active hooks for phase <p>` / `no active schedules for phase <p>`,
  exit 0.
- **`cron start`.** `read_cron_timeout_ms` (`CRON_TIMEOUT_MS`, else
  `TIMEOUT_MS`; the route caps at 30 000); sends `CronStart` with `stop`
  from the clap group. The expression is unchecked; `ATM_CRON_EXPR_INVALID`
  prints unchanged.
- **`list`.** `--json` a bare array (`[]` when empty), human `none`;
  `STOP` is `until <rfc3339 UTC>` or `hook`.
- **Task commands.** `HookActions` prints `hook ran <n> actions` (D-b); a
  refusal prints the hook's message, exit 3.

Composition (design §9.9):

```rust
// crates/atm-daemon-bootstrap/src/replacement_handler.rs
// compose_storage_router: runner = Arc::new(DaemonHookRunner::new(&env)); base router .with_hook_runner(runner.clone()),
// .with_local_host(daemon_atm_config(&env).and_then(|c| c.local_host)); snapshot = ActiveHookSnapshot::load(store)? (an error aborts startup);
// dispatcher = Arc::new(base.unhooked()); hook_filter = base.compose_hook_filter(snapshot, runner.clone(), dispatcher.clone());
// handler = base.with_hook_filter(hook_filter.clone()); the named ReplacementHandler returns this same `hook_filter` Arc for composition tests.
pub(crate) struct ReplacementHandler {
    pub(crate) handler: Arc<StorageAndNudgeRouter>,
    pub(crate) recovery_sweep: RecoverySweepHandle,
    pub(crate) runner: Arc<dyn HookRunner>,
    pub(crate) dispatcher: Arc<UnhookedRouter>,
    pub(crate) hook_filter: Arc<HookedTaskCommands>,
}
pub(crate) fn build_replacement_handler(..) -> Result<ReplacementHandler, AtmError>;

// crates/atm-daemon-bootstrap/src/cron_composition.rs: called by the daemon path and the in-crate tests; the tests bind no listener
/// AtmCron::new(deps) then handler.install_cron_scheduler(cron.clone()); the daemon passes SystemClock and SystemJitter, tests ManualClock and SeqJitter.
pub(crate) fn compose_cron(handler: &StorageAndNudgeRouter, deps: atm_cron::AtmCronDeps) -> Result<Arc<atm_cron::AtmCron>, AtmError>;
/// workers.cron = Some(cron.start_worker().await?); an initial active_all error is returned and aborts startup.
pub(crate) async fn start_cron_worker(cron: &Arc<atm_cron::AtmCron>, workers: &mut DaemonWorkers) -> Result<(), AtmError>;
/// Steps in order: "cron_worker" (CronWorkerHandle::shutdown), "hook_chains" (hook_chains().begin_shutdown() then drain(deadline)),
/// "listener" (awaits `listener`), then today's steps unchanged.
pub(crate) async fn run_shutdown_steps(listener: impl Future<Output = Result<(), AtmError>>, handler: &StorageAndNudgeRouter, workers: DaemonWorkers,
    deadline: tokio::time::Instant) -> Result<(), AtmError>;

let cron = compose_cron(&handler, atm_cron::AtmCronDeps { store: runtime.cron_schedule_store()?, runner: hook_runner.clone(),
    dispatcher: base_dispatcher.clone() as Arc<dyn HookActionDispatcher>, clock: Arc::new(atm_cron::SystemClock), jitter: Arc::new(atm_cron::SystemJitter), host: daemon_atm_config(&env).and_then(|c| c.local_host) })?;
// after start_replacement_runtime_or_abort (:503) returns Ok and before emit_ready_signal_if_requested (:624):
start_cron_worker(&cron, &mut workers).await?;
// shutdown_replacement_daemon (:805): run_shutdown_steps(async move { running.begin_shutdown().finish(deadline).await }, handler, workers, deadline)
```

`docs/user-documents/hooks.md` sections, in order:

1. "What a hook is": a filter in front of one `atm task` subcommand with
   four results (handled, pass-through, not-applicable, refused).
2. "The request": the design §2.1 JSON (`request` the command's HTTP
   body, `custom` the `--custom` object) with the §2.1 `/jev-goal` example
   of one module serving many schedules.
3. "The reply": the four design §2.2 shapes, the old `success` key being malformed, actions over any command except the
   hook and cron registry, `as`, the D-c and D-d rules, and what the CLI
   prints.
4. "The module":
   - `TIMEOUT_MS` with its grammar and the 2 500 ms cap, and the optional
     `CRON_TIMEOUT_MS` for cron ticks, capped at 30 000 ms; the daemon
     refuses a value over the cap at registration.
   - The design §9.8 `__main__` block.
   - A 40-line example implementing `task.close` and `task.assign` only.
     Every other `task.*` replies `{"result": "not-applicable"}`.
   - `cron(args)`, `stop(args)` and `doctor(request)`, with the design
     §9.10 doctor request and reply JSON and a `doctor` example that
     returns one `warning` finding.
   - A module returns actions rather than calling `atm` for its own hooked
     command; a nested `atm` call is bounded by the hook timeout.
5. "Errors you will see": the six codes and their exit codes.
6. "Registering": any roster member; many phases may register one hook,
   run oldest first until one handles (D-5). A module answers
   not-applicable quickly for a task it does not own.
7. "Cron":
   - The D-g grammar (UTC), example `0 */5 * * * *`.
   - No replay, one run in flight; fires at the occurrence plus a fixed
     random `jitter_s` (§5 D-r), shown by `atm cron list`.
   - The mandatory stop: `--until` or `--until-hook`, with the atm-bd
     phase-bead pattern.
   - `--arg`, and `atm cron end`.
8. "Doctor": one call per module, cwd, team and phase; findings under
   `hooks`, never changing the summary.
9. "Platforms": Windows `python` (via `PATHEXT`), needs `SystemRoot`,
   `USERPROFILE`; unix `PATH`, `HOME`. A Windows timeout kills only the
   child; unix kills the process group.

## Authoritative deliverables

1. The two commands and the rules above (ADR-068 D3, ADR-069 D1).
2. `read_timeout_ms` per design §9.8 (ANC-011); register hashes no module
   (D-2, ANC-016) and runs no `bd` (ANC-010).
3. `exit_code_for_atm_error` arms per design §9.1: the three validation
   codes exit 3; `HookExit`, `HookTimeout`, `HookMalformed` exit 1 through
   `_ => 1`.
4. `print_doctor_hooks` (bk-7 collects the entries) renders `Hooks:`, a
   line per entry
   (`<team>/<phase> <module>: <message>` or the error), an indented
   `<severity> <code>: <message>` per finding, or `Hooks: none`.
5. Composition and shutdown as above (`REQ-P-CRON-002`, ADR-069 D2);
   the `atm-runtime` store wiring.
6. The user documentation above; `cli-reference.md` lists every flag.
7. Tests, no live daemon (REQ-BK-TESTS).
   - CLI tests in `hook.rs`/`cron.rs` `mod tests` over
     `CliComposition::from_fake_transport` (`composition.rs:207`, pattern
     `send.rs:2680`); the `FakeClientTransport` handler records each
     `ApiRequest` and returns a scripted response. They assert the built
     request, refusals before any request, printing and exit codes; a
     route-side refusal is scripted.
   - `hook_cron_composition_tests.rs`: the `build_replacement_handler`
     handler on a temp-dir store plus `compose_cron`, `ManualClock`,
     `SeqJitter`, no listener. A real child through `DaemonHookRunner`
     runs in a plain `#[tokio::test]`. Synchronisation: `before_reply` cannot hold a real
     child, so each such test makes exactly one `ManualClock::advance` to
     one occurrence, then calls `CronWorkerHandle::shutdown` with a 60 s
     deadline, which joins the in-flight run; the files the fixture wrote
     and the row are read only after that join. No sleep, no polling.

## Authoritative acceptance criteria

`crates/atm/src/commands/hook.rs::tests` (fake transport; "counter" is its recorded request count):

- [ ] (ANC-011, REQ-BK-HOOK-CLI) `req:REQ-P-HOOK-003` `register_reads_timeout_ms_from_module`: `TIMEOUT_MS = 2500` sends one `HookRegister`, `timeout_ms == 2500`, canonical module and cwd; `register_refuses_bad_timeout_ms_before_any_request`: `no_timeout.py`, `zero_timeout.py`, `indented.py`, `two_lines.py` exit 3, `ATM_HOOK_MODULE_INVALID`, counter 0; `register_over_cap_sends_and_prints_the_daemon_refusal`: `over_cap.py` sends `timeout_ms == 3000` once, the scripted route error printed unchanged, exit 3; `register_refuses_missing_module_and_missing_cwd_before_any_request` (exit 3, `ATM_HOOK_MODULE_INVALID`, counter 0); `hook_module.rs::tests::timeout_ms_grammar` covers `TIMEOUT_MS: int = 2500  # ms` accepted and `TIMEOUT_MSX = 1` refused.
- [ ] (ANC-013) `req:REQ-P-HOOK-003` `register_sends_any_hook_name_and_prints_the_daemon_message`: `cron.tick`, `task.foo` sent unchanged, the scripted error printed, exit 3.
- [ ] (ANC-002, ANC-010, REQ-BK-HOOK-CLI) `req:REQ-P-HOOK-003` `disable_sends_phase_and_hook_and_prints_one_line_per_row` (`atm hook disable --phase bk task.close` sends `HookDisable { phase: bk, hook: Some(task.close) }`); `disable_with_no_active_hooks_prints_none_and_exits_zero`; `list_prints_inactive_rows_and_json_is_a_bare_array` (inactive row: `ACTIVE` false, `DISABLED_AT`; no rows: `[]`); the flags of `hook register` and `cron start` are carried by the `cli_surface` baseline (REQ-BK-HOOK-CLI).
- [ ] (REQ-BK-ERRORS, ANC-004) `req:REQ-P-HOOK-003` `crates/atm/src/main.rs::tests::hook_and_cron_codes_map_to_documented_exit_codes`: the six codes give 3, 3, 3, 1, 1, 1 (§9.1 order); `output.rs::tests::hook_actions_response_prints_hook_ran_n_actions`.

`crates/atm/src/commands/cron.rs::tests` (same transport):

- [ ] (ANC-019, D-j, REQ-BK-HOOK-CLI) `req:REQ-P-CRON-001` `start_sends_args_timeout_and_stop`: `--arg status --arg --json --until-hook` sends one `CronStart`, `args == ["status","--json"]`, `cron_expr` byte-identical, `timeout_ms == 2500`, `stop == Hook`; `list_prints_stop_until_in_utc`; `cron_list_empty_prints_none_and_json_brackets`; `cron_end_with_nothing_active_prints_none_and_exits_zero` (`no active schedules for phase bk`); `start_refuses_missing_module_and_missing_cwd_before_any_request` (exit 3, `ATM_HOOK_MODULE_INVALID`, counter 0).
- [ ] (Rand 2026-10-18 cron timeout) `req:REQ-P-CRON-001` `cron_timeout_defaults_to_timeout_ms` (`valid_hook.py` without `CRON_TIMEOUT_MS` sends `timeout_ms == 2500`; `cron_timeout.py` sends `20000`); `cron_start_over_cap_sends_and_prints_the_daemon_refusal` (`cron_over_cap.py` sends `timeout_ms == 30001` once; scripted route error, exit 3); `hook register` of `cron_timeout.py` sends `timeout_ms == 2500`.
- [ ] (ANC-022, Rand 2026-10-18 custom) `req:REQ-P-CRON-001` `custom_flag_reaches_the_request`: `--custom '{"goal":"ship"}'` is the sent `HookRegister`'s and `CronStart`'s `custom`; `list --json` carries `custom`, `jitter_s`, the table `1 keys`, `JITTER_S`; `custom_non_object_refused`: `'[1]'`, `'x'` are clap errors naming `--custom`, counter 0.
- [ ] (ANC-023) `req:REQ-P-CRON-001` `start_refuses_{missing,both}_stop_condition{,s}_before_any_request` (clap usage error, counter 0).

`crates/atm/src/output/hooks.rs::tests` and `crates/atm-runtime`:

- [ ] (REQ-P-HOOK-004) `req:REQ-P-HOOK-004` `doctor_human_output_renders_hooks_findings_and_none`: `Hooks:`, `bk/bk <module>: ok`, the indented `warning <code>: <message>`, an error entry's error, and `Hooks: none` when empty.
- [ ] `req:REQ-P-HOOK-001` `crates/atm-runtime/src/composition.rs::tests::composition_installs_the_hook_and_cron_stores` (pattern `:347`).
- [ ] (D-2, ANC-016) `req:REQ-P-HOOK-004` `rg -n 'sc_sha|sha256|released_sha|sha1' crates/atm/src/commands/hook.rs crates/atm/src/commands/cron.rs crates/atm/src/output/hooks.rs` and `rg -nw 'bd|\.beads'` on the same files are empty.

`crates/atm-daemon-bootstrap/src/hook_cron_composition_tests.rs` (composed handler, `compose_cron`, no listener):

- [ ] (ANC-019, ANC-023, D-5) `req:REQ-P-CRON-001` `start_persists_row_with_args_and_timeout` (row = spec plus `jitter_s`); `list_shows_last_run_and_outcome_after_record_run`; `start_refuses_until_in_the_past_with_daemon_message`; `start_refuses_invalid_expression_with_cron_expr_invalid` (`x y`, five-field `* * * * *`; list empty); `start_refuses_active_duplicate_name`; `end_phase_keeps_rows_inactive_with_ended_at`; `register_second_phase_same_hook_succeeds` (`bj`'s `task.close` active, `bk`'s registers; `HookList` shows both).
- [ ] (ANC-021, REQ-BK-D3) `req:REQ-P-HOOK-001` `filter_and_cron_share_one_runner_and_one_dispatcher`: the returned `ReplacementHandler` exposes its composed `hook_filter`; its runner and dispatcher are `Arc::ptr_eq` to the filter's `runner()` and `actions()` and to `AtmCronDeps`; `snapshot_load_error_fails_daemon_composition` (`fail_active_all`); `snapshot_load_from_reopened_store_serves_pre_restart_registrations` (`bj`, `bk` on `task.close`, reopened temp-dir store: `[bj, bk]`).
- [ ] (ANC-004) `req:REQ-P-HOOK-001` `close_end_to_end_with_refusing_hook_leaves_task_open`: `fixture_hook.py` on `task.close`, `timeout_ms = 30000`, stored before composition; a close gives `ATM_HOOK_REFUSED` with the module's message, task still `active`.
- [ ] (ANC-008, ANC-021) `req:REQ-P-CRON-002` `cron_tick_action_runs_unhooked`: a schedule runs `fixture_hook.py` in cwd A (lists `cron.tick`; replies one `task.assign` action) while cwd B's is registered for `task.assign`; after one `ManualClock::advance` and `shutdown` (which joins the run) the task exists and B's `fixture_request.json` is never written.
- [ ] (ADR-069 D2) `req:REQ-P-CRON-002` `cron_scheduler_installed_once_and_cron_routes_reach_it` (a second `install_cron_scheduler` errors); `cron_worker_is_first_shutdown_step`: `run_shutdown_steps` with an `async { Ok(()) }` listener records steps beginning `cron_worker`, `hook_chains`, `listener`; `cron_initial_load_error_aborts_startup`: `start_cron_worker` with `fail_active_all` returns `Err`, `workers.cron` `None`.
- [ ] (REQ-BK-D4A) `req:REQ-P-CRON-002` `daemon_restart_with_schedule_due_while_down_fires_once_at_next_occurrence_and_not_at_startup`: `ManualClock`, `DummyHookRunner`; a second `compose_cron` on the reopened store at 10:00:30 for a 10:00:00 occurrence makes no call before an advance, one at the next occurrence.
- [ ] (ANC-017, ANC-024) `req:REQ-P-CRON-002` `fixture_cron_receives_args_through_the_composed_scheduler` (real clock, `DaemonHookRunner`, `fixture_cron.py`, an `--until-hook` schedule with `timeout_ms = 30000`): after an advance and `shutdown`, `cron.stop` then `cron.tick` (with `request.args`) are written, each `sys.argv[1:]` its hook name only, `last_outcome == "ok"`; with the stop marker, a second `start_cron_worker`, advance and `shutdown` give `"stopped: marker present"`, `active == false`.
- [ ] (ANC-015, REQ-P-HOOK-004) `req:REQ-P-HOOK-004` `hook_cron_composition_tests.rs::doctor_runs_the_registered_module_doctor_for_its_team` (real clock, `DaemonHookRunner`): `fixture_hook.py` stored for team `bk`, phase `bk`, `custom` `{"goal":"ship"}`, replying `{"success":true,"message":"ok","findings":[one warning]}`; doctor for `bk` leaves `fixture_request.json` with that team, phase and `custom`, and one hooks entry with that team, phase, message and warning; doctor for `other` runs no module.

Surface and docs:

- [ ] (REQ-BK-HOOK-CLI) `req:REQ-P-HOOK-003` `cargo test -p atm --test cli_surface` passes, baseline diff only the `hook` and `cron` subtrees; `crates/atm/Cargo.toml` unchanged.
- [ ] `req:REQ-P-USER-DOCS-001` `hooks.md`'s example handles `task.close`, `task.assign` only and its post-send sections remain; `cli-reference.md` has `` ### `atm cron` `` and `` ### `atm hook` ``; `just lint` (site-links) passes.
- [ ] `req:REQ-P-USER-DOCS-002` `hooks.md` front matter keeps `title`, `audience`, `reviewed_for_release` (the release version); a `python3 -c` over `hooks.md` and `tasks.md` runs `json.loads` on every fenced `json` block without error.
- [ ] (ANC-011) `req:REQ-P-CRON-001` `cron.rs::tests::until_parses_rfc3339_and_normalizes_to_utc` (`2026-11-01T02:00:00+02:00` gives `2026-11-01T00:00:00Z`; `2026-11-01`, `tomorrow` are clap errors naming `--until`, counter 0).

## Authoritative validation

- `cargo test -p atm -p atm-runtime -p atm-daemon-bootstrap`
- `cargo clippy -p atm -p atm-runtime -p atm-daemon-bootstrap --all-targets -- -D warnings`
- `just lint` (boundaries, site-links, `.just/check_line_counts.py`)
- `just test`

## Non-closure

- The live-host walkthrough is the phase-ending gate's evidence.
