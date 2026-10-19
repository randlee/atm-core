# Sprint bk-5 — hook and cron CLI, doctor hooks section, user docs

closure_type `integration`; target_boundary `BOUNDARY-AtmCli` (`atm hook`,
`atm cron`, `atm doctor` hooks section) over `req:REQ-P-HOOK-003`,
`req:REQ-P-HOOK-004`, `req:REQ-P-CRON-001`, `req:REQ-P-CRON-002`
(composition); branch `sprint/bk-5-hook-cli-doctor`, PR → the stack top
above bk-4 (`sprint/bk-4-atm-cron`, after bk-4 is rebased onto bk-3);
difficulty normal. Consumes bk-2: the six request and six response
envelope variants, `HookRegistration`, `CronSchedule`, `HookName`,
`PhaseId`, the `ATM_HOOK_*`/`ATM_CRON_*` codes, `HTTP_API_VERSION 1.13.0`,
all through `atm_core::boundary`; bk-3: the six daemon routes,
`HookRunner::doctor` through the router's `hook_runner` field,
`with_cron_scheduler`, `DaemonHookRunner`, the router's
`HookActionDispatcher` impl; bk-4: `AtmCron`, `AtmCronDeps`,
`CronWorkerHandle`, `SystemClock`. Governing text: `design.md` §2.3, §5,
§6, D-2, D-4, D-5; anchors ANC-010, ANC-011, ANC-013, ANC-015, ANC-016, ANC-019,
REQ-BK-HOOK-CLI, REQ-BK-TESTS. Requirements `REQ-P-HOOK-003` §23.1,
`REQ-P-HOOK-004` §11.5, `REQ-P-CRON-001` §24.1, `REQ-P-HOOK-001` §23.2
(module contract read by the CLI), `REQ-P-USER-DOCS-001`; ADR-068 D3, D5,
D6, ADR-069 D1, ADR-066 (team from caller context).

## Scope boundary

This sprint owns the user-facing surface: the two new CLI commands, the
`TIMEOUT_MS` read, the `hooks` section of `atm doctor` across `atm-core`
(report type), `atm-http-runtime` (the router's `doctor()` only) and `atm`
(rendering), the CLI surface baseline, the user documentation, and the
daemon composition of `atm-cron` (the integration step: `AtmCron` built in
`run_replacement_daemon`, injected into the router, started and drained as
a daemon worker). It creates no storage, protocol, runner, route or
scheduler code and edits no file under `crates/atm-cron` or any bk-3 file
but the router's `doctor()` and the bootstrap's composition function.

Owned paths (new unless marked edited):

- `crates/atm/src/commands/hook.rs`, `crates/atm/src/commands/cron.rs`,
  `crates/atm/src/commands/hook_module.rs` (the `TIMEOUT_MS` read),
  `crates/atm/src/commands/mod.rs` (edited: `Command::Hook(HookCommand)`,
  `Command::Cron(CronCommand)` in the enum at line 118 and their dispatch
  arms), `crates/atm/src/output.rs` (edited: `print_hook_list`,
  `print_cron_list`, hooks block in `print_doctor_result` at line 288),
- `crates/atm/tests/cli_surface_baseline.json` (edited: the two commands
  and their flags), `crates/atm/tests/hook_cli.rs`,
  `crates/atm/tests/cron_cli.rs`, `crates/atm/tests/fixtures/hook_module/`
  (`valid_hook.py`, `no_timeout.py`, `zero_timeout.py`)
- `crates/atm-core/src/doctor/hooks.rs`, `crates/atm-core/src/doctor/mod.rs`
  (edited: `pub mod hooks`), `crates/atm-core/src/doctor/report.rs` (edited:
  `hooks` field on `DoctorReport` at line 467)
- `crates/atm-http-runtime/src/doctor_hooks.rs`,
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs` (edited: the
  `doctor()` function at line 651 only, one call into `doctor_hooks`),
  `crates/atm-http-runtime/src/lib.rs` (edited: `mod doctor_hooks`)
- `crates/atm-daemon-bootstrap/src/lib.rs` (edited: `DaemonWorkers`
  field `cron: atm_cron::CronWorkerHandle` at the struct on lines 532-539,
  `for_process` argument, `run_replacement_daemon` composition below,
  `shutdown_replacement_daemon` step at lines 805-834),
  `crates/atm-daemon-bootstrap/Cargo.toml` (edited: `atm-cron.workspace =
  true`), `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`
  (edited: `atm-cron` in `allowed_dependencies`, `cron_worker_lifecycle` in
  `io_owns`), `.just/lint-config.toml` (edited: `atm-cron` in the
  `crates/atm-daemon-bootstrap/Cargo.toml` allowlist),
  `crates/atm-daemon-bootstrap/tests/cron_fixture/fixture_cron.py` (new)
- `docs/user-documents/cli-reference.md` (edited: `### atm hook`,
  `### atm cron` in alphabetical position, hooks paragraph under
  `### atm doctor` at line 109), `docs/user-documents/tasks.md` (edited:
  "Task hooks" section), `docs/user-documents/hooks.md` (new: the module
  author's guide)

## Public contract

```rust
// crates/atm/src/commands/hook.rs  (clap, pattern of task.rs:41)
#[derive(Debug, Parser)] pub struct HookCommand { #[command(subcommand)] pub command: HookSubcommand }
#[derive(Debug, Subcommand)]
pub enum HookSubcommand { Register(HookRegisterArgs), Disable(HookDisableArgs), List(HookListArgs) }
#[derive(Debug, Args)] pub struct HookRegisterArgs {
    pub hook: String,                                  // sent as typed; the daemon's register route is the only validator (HookName::parse_task_hook, ADR-068 D3) and its ATM_HOOK_MODULE_INVALID message is printed unchanged
    #[arg(long)] pub module: PathBuf, #[arg(long)] pub phase: String, #[arg(long)] pub cwd: Option<PathBuf> }   // cwd default: module's parent directory
#[derive(Debug, Args)] pub struct HookDisableArgs { #[arg(long)] pub phase: String, pub hook: Option<String> }
#[derive(Debug, Args)] pub struct HookListArgs { #[arg(long)] pub phase: Option<String>, #[arg(long)] pub json: bool }
impl HookCommand { pub async fn run(self, observability: &CliObservability) -> Result<()>; }
// Register: preflight_daemon_api(composition, HttpApiVersion::parse("1.13.0")?, "hook register") (send.rs:717), read_timeout_ms(&module),
//   canonicalize module and cwd, execute_request(RequestEnvelope::HookRegister(HookRegisterRequest { caller_identity, caller_team, phase, hook, module_path, cwd, timeout_ms })),
//   print "registered <hook> for <team>/<phase> (timeout <n> ms)"; ATM_HOOK_ACTIVE from the daemon is printed as the daemon's message (names the other phase).
// Disable: RequestEnvelope::HookDisable -> prints one line per row changed. List: RequestEnvelope::HookList -> print_hook_list(rows, json).

// crates/atm/src/commands/hook_module.rs
/// Reads the file, finds exactly one line that, after trimming, starts with `TIMEOUT_MS`, then optional spaces, `=`, optional spaces, and ends with
/// ASCII digits only (std string ops: `trim`, `strip_prefix`, `char::is_ascii_digit`; no regex crate), returns the integer; Err(ATM_HOOK_MODULE_INVALID)
/// when the file is missing or unreadable, no line matches, more than one matches, the value is 0 or exceeds u32::MAX. The read stays CLI-side (ADR-068 D3).
pub fn read_timeout_ms(module: &Path) -> Result<u32, AtmError>;

// crates/atm/src/commands/cron.rs
#[derive(Debug, Parser)] pub struct CronCommand { #[command(subcommand)] pub command: CronSubcommand }
#[derive(Debug, Subcommand)] pub enum CronSubcommand { Start(CronStartArgs), End(CronEndArgs), List(CronListArgs) }
#[derive(Debug, Args)]
#[command(group = clap::ArgGroup::new("stop").required(true).multiple(false).args(["until", "until_hook"]))]   // design D-9: exactly one stop condition; clap refuses none or both before any request
pub struct CronStartArgs {
    pub name: String, #[arg(long = "cron")] pub cron_expr: String, #[arg(long)] pub module: PathBuf, #[arg(long)] pub phase: String,
    #[arg(long, value_parser = parse_rfc3339_utc)] pub until: Option<chrono::DateTime<chrono::Utc>>,   // --until 2026-11-01T00:00:00Z
    #[arg(long = "until-hook")] pub until_hook: bool,                                                   // --until-hook: the module's stop(args) decides
    #[arg(long, num_args = 0.., allow_hyphen_values = true)] pub args: Vec<String>, #[arg(long)] pub cwd: Option<PathBuf> }
fn parse_rfc3339_utc(s: &str) -> Result<chrono::DateTime<chrono::Utc>, String>;   // chrono::DateTime::parse_from_rfc3339 then .with_timezone(&Utc); the clap error names the flag
// stop = match (until, until_hook) { (Some(at), false) => StopCondition::Until { at }, (None, true) => StopCondition::Hook, _ => unreachable (clap group) }
#[derive(Debug, Args)] pub struct CronEndArgs { #[arg(long)] pub phase: String, pub name: Option<String> }
#[derive(Debug, Args)] pub struct CronListArgs { #[arg(long)] pub phase: Option<String>, #[arg(long)] pub json: bool }
impl CronCommand { pub async fn run(self, observability: &CliObservability) -> Result<()>; }
// Start: preflight 1.13.0, read_timeout_ms, RequestEnvelope::CronStart(CronStartRequest {...}); the daemon's scheduler parses the expression (bk-3 route, bk-4 AtmCron) and the CLI prints ATM_CRON_EXPR_INVALID's message unchanged.

// crates/atm/src/commands/mod.rs additions
Command::Hook(HookCommand), Command::Cron(CronCommand)   // `atm hook`, `atm cron`

// crates/atm/src/output.rs additions
pub fn print_hook_list(rows: &[HookRegistration], json: bool) -> Result<()>;   // table: PHASE HOOK ACTIVE MODULE CWD TIMEOUT_MS REGISTERED_BY REGISTERED_AT DISABLED_AT; active rows first
pub fn print_cron_list(rows: &[CronSchedule], json: bool) -> Result<()>;       // table: PHASE NAME ACTIVE CRON MODULE ARGS TIMEOUT_MS STOP LAST_RUN_AT LAST_OUTCOME; STOP is "until <rfc3339>" or "hook"
// print_doctor_result: after the roster, a "Hooks" block: one line per entry `"<phase>/<hook> <module_path>: ok <reply json>"` or `"... : <error code> <message>"`; "Hooks: none" when empty.

// crates/atm-core/src/doctor/hooks.rs
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDoctorEntry {
    pub phase: PhaseId, pub hook: HookName, pub module_path: PathBuf, pub cwd: PathBuf, pub timeout_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub reply: Option<serde_json::Value>,   // doctor() JSON on success
    #[serde(default, skip_serializing_if = "Option::is_none")] pub error: Option<HookDoctorError>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDoctorError { pub code: AtmErrorCode, pub message: String }
// crates/atm-core/src/doctor/report.rs addition
pub struct DoctorReport { /* existing fields unchanged */ #[serde(default, skip_serializing_if = "Vec::is_empty")] pub hooks: Vec<HookDoctorEntry> }

// crates/atm-http-runtime/src/doctor_hooks.rs
/// `store.list(team, None)` filtered to active rows, in (phase, hook) order; for each `runner.doctor(&reg).await` under the request deadline;
/// Ok(v) => reply = Some(v); Err(e) => error = Some({code, message}). Never returns Err: a store failure yields an empty Vec and one ADR-067
/// entry (`subsystem = "atm_hook"`, `action = "doctor"`, `outcome = "store_error"`). One entry per run with `outcome = "ok" | <code>`.
pub(crate) async fn collect(router: &StorageAndNudgeRouter, team: &TeamName, deadline: RequestDeadline) -> Vec<HookDoctorEntry>;
// storage_and_nudge_router.rs `doctor()` (line 651): after the report is built and before `Ok(ApiResponse::new(ResponseEnvelope::Doctor(...)))`,
//   `report.hooks = doctor_hooks::collect(self, &team, deadline).await` when the query resolves to one team (caller team, ADR-066); empty for all-teams scope.

// crates/atm-daemon-bootstrap/src/lib.rs composition (run_replacement_daemon), after the router is built with with_hook_runner and with_local_host (bk-3):
//   let cron = Arc::new(atm_cron::AtmCron::new(atm_cron::AtmCronDeps { store: runtime.cron_schedule_store()?, runner: hook_runner.clone(),
//       dispatcher: router.clone() /* Arc<StorageAndNudgeRouter> as Arc<dyn HookActionDispatcher> */, clock: Arc::new(atm_cron::SystemClock),
//       observability: observability.clone(), host: daemon_atm_config(&env).and_then(|c| c.local_host) }));   // atm_temp_config.rs:20, the same config read the sweeper uses
//   let router = router.with_cron_scheduler(cron.clone());   // the three cron routes now reach AtmCron
//   workers.cron = cron.start_worker();                      // loads active_all() (ANC-020) and begins ticking
//   shutdown_replacement_daemon: shutdown_probe::step("cron_worker", deadline, workers.cron.shutdown(deadline)) before "atm_temp_sweeper".
```

`docs/user-documents/hooks.md` sections, in order: "What a hook is" (one
paragraph: replaces the subcommand, three replies, never recursive); "The
request" (the §2.1 JSON with every field explained, the `request` field as
the envelope JSON for each of the six hooks); "The reply" (three shapes,
`as`, action `request`); "The module" (`TIMEOUT_MS`, `hook(request)`,
`cron(args)`, `stop(args)`, `doctor()`, a 30-line example module that
refuses `task.close` when a file is missing and otherwise passes through,
and whose `stop()` returns stop once a marker file exists); "Errors you
will see" (the six codes); "Registering" (`atm hook register|disable|list`,
open to every roster member of the team, with the
one-active-per-team-and-hook rule); "Cron" (`atm cron start|end|list`, the
7-field seconds-first `cron` dialect with `0 */5 * * * * *` as the example,
UTC, no missed-tick replay, one run in flight, the mandatory stop condition
`--until <rfc3339>` or `--until-hook` with the `stop()` reply shapes and
the atm-bd pattern of stopping when the phase bead closes, `atm cron end`
as the manual stop); "Doctor" (what `atm doctor` prints for a registration).

## Authoritative deliverables

1. `atm hook register|disable|list` and `atm cron start|end|list` as
   above, sending the bk-2 envelopes through the existing CLI client
   (`execute_request`, pattern `task.rs:657`) after
   `preflight_daemon_api(.., "1.13.0", ..)`; the team is the caller's
   context team (`ATM_TEAM` or `--team`, ADR-066), never read from the
   daemon. The CLI validates nothing about the hook name or the cron
   expression: both messages come from the daemon and are printed
   unchanged (ADR-068 D3, ADR-069 D1).
1a. Daemon composition of `atm-cron` as in the contract: `AtmCron` built
   once in `run_replacement_daemon` from the runtime's
   `cron_schedule_store()`, the shared `DaemonHookRunner`, the router as
   `HookActionDispatcher`, `SystemClock` and the daemon's host name;
   injected with `with_cron_scheduler`; `DaemonWorkers.cron` holds the
   `CronWorkerHandle` and `shutdown_replacement_daemon` drains it as one
   `shutdown_probe::step` under the same deadline, before
   `atm_temp_sweeper` (`REQ-P-CRON-002`, ADR-069 D2).
2. `read_timeout_ms` as above (`REQ-P-HOOK-001` §23.2: a module without
   `TIMEOUT_MS` is refused before any request is sent; ANC-011).
3. `atm hook list` prints every row of the team (active first, then
   disabled with `DISABLED_AT`); `atm cron list` prints `STOP`,
   `LAST_RUN_AT` and `LAST_OUTCOME` (ANC-019, `REQ-P-CRON-001`). `atm cron
   start` requires exactly one of `--until` and `--until-hook` through the
   clap group above and sends it as `stop` (ANC-023).
4. `HookDoctorEntry`, `HookDoctorError`, `DoctorReport.hooks` with serde
   default (a pre-bk JSON report still decodes, `REQ-P-HOOK-004`);
   `doctor_hooks::collect` and the one-line call in the router's `doctor()`;
   the section never fails doctor and never changes `summary` (ANC-015,
   ANC-016).
5. `print_doctor_result` renders the block in human mode and the `--json`
   report carries `hooks` (ANC-015).
6. `cli_surface_baseline.json` regenerated with `DumpCliSurface` for the two
   commands (the surface test fails otherwise), reviewed in the PR as the
   only baseline change.
7. User documentation: `hooks.md` as above; `cli-reference.md` gets `### atm
   cron` and `### atm hook` entries in the file's alphabetical order with
   every flag, and a "Hooks" paragraph under `### atm doctor`;
   `tasks.md` gets a "Task hooks" section pointing to `hooks.md` and
   stating that a refused hook leaves the task untouched.
8. Tests, same unit, through the in-process harness of
   `crates/atm/tests/task_ledger_cli.rs` (`open_isolated_sqlite_boundary`,
   `LocalServiceRuntime`, the router with `DummyHookRunner` and the real
   rusqlite stores; no live daemon, REQ-BK-TESTS):
   `hook_cli.rs`: `register_reads_timeout_ms_from_module`,
   `register_refuses_module_without_timeout_ms`,
   `register_refuses_zero_timeout_ms`,
   `register_refuses_cron_tick_name`,
   `register_refuses_second_phase_for_same_hook_naming_the_phase` (D-5),
   `disable_phase_keeps_rows_and_list_shows_them_inactive`,
   `list_json_matches_registration_rows`,
   `doctor_hooks_section_lists_active_registrations_with_doctor_reply`,
   `doctor_hooks_section_reports_timeout_without_failing_doctor`,
   `doctor_hooks_section_is_empty_for_team_without_registrations`,
   `doctor_report_without_hooks_field_still_decodes`,
   `close_end_to_end_with_refusing_hook_leaves_task_open` (a registered
   `fixture_hook.py` from bk-3 through `DaemonHookRunner`, the one
   end-to-end criterion: `atm task close` refused by the hook, the task
   still `active`); `crates/atm-daemon-bootstrap/tests/cron_composition.rs`:
   `fixture_cron_receives_args_through_the_composed_scheduler` (an
   `AtmCron` composed exactly as `run_replacement_daemon` does, with
   `DaemonHookRunner` and `fixture_cron.py` echoing its stdin: `run_now`
   on a schedule started with `--until-hook` first writes the `cron.stop`
   request, then (its `stop()` replying `{"stop": false}`) the `cron.tick`
   request with `request.args`, to the fixture's output file, and
   `last_outcome == "ok"`; a second `run_now` after the fixture's
   stop-marker file is created ends the schedule with `last_outcome ==
   "stopped: marker present"`),
   `daemon_workers_drain_cron_handle_under_deadline`; `cron_cli.rs`: `start_persists_row_with_args_and_timeout`,
   `start_refuses_invalid_expression_with_cron_expr_invalid`,
   `start_refuses_active_duplicate_name`,
   `start_refuses_missing_stop_condition_before_any_request`,
   `start_refuses_both_stop_conditions_before_any_request`,
   `start_with_until_stores_until_at_and_list_shows_it`,
   `start_with_until_hook_stores_stop_hook`,
   `end_phase_keeps_rows_inactive_with_ended_at`,
   `list_shows_last_run_and_outcome_after_record_run`.

## Authoritative acceptance criteria

- [ ] (ANC-011) `req:REQ-P-HOOK-001` `atm hook register task.close --module no_timeout.py --phase bk` exits non-zero with `ATM_HOOK_MODULE_INVALID` and the daemon receives no request (router request counter 0); with `valid_hook.py` (`TIMEOUT_MS = 2500`) the stored row has `timeout_ms == 2500`.
- [ ] (ANC-013) `req:REQ-P-HOOK-003` `atm hook register cron.tick ...` and `atm hook register task.foo ...` exit non-zero printing the daemon's `ATM_HOOK_MODULE_INVALID` message; `rg -n 'parse_task_hook|TASK_HOOKS' crates/atm/src` is empty (the daemon is the only validator).
- [ ] (D-5) `req:REQ-P-HOOK-003` registering `task.close` for phase `bk` while `bj`'s is active prints a message containing `bj` and exits non-zero; after `atm hook disable --phase bj` it succeeds.
- [ ] (ANC-010) `req:REQ-P-HOOK-003` `atm hook disable --phase bk` prints the six rows it changed and `atm hook list --json` shows them with `"active": false` and a `disabled_at`.
- [ ] (ANC-015) `req:REQ-P-HOOK-004` with one active registration whose `DummyHookRunner` doctor reply is `{"manifest":"ok"}`, `atm doctor --json` has `hooks[0].reply == {"manifest":"ok"}` and `hooks[0].hook == "task.close"`; the human output prints the `Hooks` block.
- [ ] (ANC-016) `req:REQ-P-HOOK-004` a doctor reply of `Err(ATM_HOOK_TIMEOUT)` yields `hooks[0].error.code == "ATM_HOOK_TIMEOUT"`, the exit status and `summary` equal those of the same database with no registration.
- [ ] (D-2) `req:REQ-P-HOOK-004` no field of `HookRegistration`, `CronSchedule`, `HookDoctorEntry` or any `atm hook`/`atm cron` flag names a sha (`rg -i 'sha' crates/atm/src/commands/hook.rs crates/atm/src/commands/cron.rs crates/atm-core/src/doctor/hooks.rs` is empty).
- [ ] (ANC-019) `req:REQ-P-CRON-001` `atm cron start nightly --cron "0 0 2 * * * *" --module valid_hook.py --phase bk --until-hook --args status --json` stores `args == ["status", "--json"]`, `cron_expr` byte-identical, `timeout_ms == 2500`, `stop == Hook`; `atm cron list --json` shows the row with `"stop": {"kind": "hook"}`; after `record_run(id, t, "ok")` the list shows `last_run_at == t` and `last_outcome == "ok"`.
- [ ] (ANC-023) `req:REQ-P-CRON-001` `atm cron start x --cron "0 * * * * * *" --module valid_hook.py --phase bk` (no stop flag) and the same with both `--until 2026-11-01T00:00:00Z --until-hook` exit non-zero with a clap usage error naming `--until`/`--until-hook`, and the router request counter is 0; with `--until 2026-11-01T00:00:00Z` alone the stored row has `stop == Until { at: 2026-11-01T00:00:00Z }` and the human list prints `until 2026-11-01T00:00:00Z` under `STOP`.
- [ ] (REQ-BK-HOOK-CLI) `req:REQ-P-CRON-001` `atm cron start x --cron "x y" ...` exits non-zero printing the daemon's `ATM_CRON_EXPR_INVALID` message and `atm cron list` is empty; `atm cron end --phase bk` keeps the rows with `ended_at` set.
- [ ] (REQ-BK-TESTS) `req:REQ-P-HOOK-001` `close_end_to_end_with_refusing_hook_leaves_task_open` passes: `atm task close` against a registered refusing fixture returns the hook's message and `atm task list` still shows the task `active`.
- [ ] (ANC-017, ANC-024, REQ-P-CRON-002) `req:REQ-P-CRON-002` `fixture_cron_receives_args_through_the_composed_scheduler` passes: the fixture's output file holds the `cron.stop` request then the `cron.tick` request with `{"args": ["status", "--json"]}` read from stdin, each with `sys.argv[1:]` equal to its hook name only; `atm cron list --json` then shows `last_outcome == "ok"`, and after the marker file `"stopped: marker present"` with `active == false`.
- [ ] (ADR-069 D2) `req:REQ-P-CRON-002` `run_replacement_daemon` builds exactly one `AtmCron`, passes it with `with_cron_scheduler` and keeps its `CronWorkerHandle` in `DaemonWorkers.cron`; `shutdown_replacement_daemon` records a `cron_worker` probe step (`rg -n '"cron_worker"' crates/atm-daemon-bootstrap/src/lib.rs` is one line).
- [ ] (ANC-011) `req:REQ-P-HOOK-001` `cargo tree -p atm -e normal -i regex` reports the crate absent (the `TIMEOUT_MS` read uses std only).
- [ ] `req:REQ-P-HOOK-003` `cargo test -p atm --test cli_surface` passes and the baseline diff contains only the `hook` and `cron` subtrees.
- [ ] `req:REQ-P-USER-DOCS-001` `docs/user-documents/hooks.md` exists with the eight sections above; `cli-reference.md` has `### atm cron` and `### atm hook`; `just lint` (site-links gate) passes.

## Authoritative validation

- `cargo test -p atm -p atm-core -p atm-http-runtime -p atm-daemon-bootstrap`
- `cargo clippy -p atm -p atm-core -p atm-http-runtime -p atm-daemon-bootstrap --all-targets -- -D warnings`
- `cargo build --workspace`
- `just lint`
- `just test` (the phase's integration sprint runs the full suite)

## Non-closure

- No scheduler or worker code is written here (bk-4 `atm-cron`); this
  sprint composes it. `atm cron list` outcomes in `cron_cli.rs` come from
  `record_run` called by the test; the one composed tick is
  `fixture_cron_receives_args_through_the_composed_scheduler`.
- No hook process is spawned except in the one end-to-end test that uses
  bk-3's runner and fixture; runner behaviour is bk-3's closure.
- The live-host walkthrough (register on `atm-dev`, close a task, read the
  doctor section) is the phase-ending gate's evidence, not this sprint's.
