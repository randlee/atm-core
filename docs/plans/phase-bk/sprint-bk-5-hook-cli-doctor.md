# Sprint bk-5 — hook and cron CLI, doctor hooks section, composition, user docs

closure_type `integration`; target_boundary the `atm` CLI (`atm hook`,
`atm cron`, the `atm doctor` hooks section) and the daemon composition of
`atm-cron`, over `req:REQ-P-HOOK-001`, `req:REQ-P-HOOK-003`,
`req:REQ-P-HOOK-004`, `req:REQ-P-CRON-001`, `req:REQ-P-CRON-002`; branch `sprint/bk-5-hook-cli-doctor`, PR → the stack top above
bk-4 (`sprint/bk-4-atm-cron`); difficulty normal; recommended_agent
`arch-ctm`, recommended_model `gpt-5-codex`. Consumes bk-1 (the seven
codes), bk-2 (the six request envelopes and their responses,
`HookActionsResponse`, `HookDoctorEntry`, `DoctorReport.hooks`,
`HookName`, `PhaseId`, `StopCondition`, `HTTP_API_VERSION 1.13.0`), bk-3
(the six routes, `X-ATM-Hook-Actions`, the router's `hook_runner` field,
`install_cron_scheduler`, the four-tuple of `build_replacement_handler`,
`fixture_hook.py`) and bk-4 (`AtmCron`, `AtmCronDeps`, `CronWorkerHandle`,
`SystemClock`, `SystemJitter`). Governing text: `design.md` §2.3, §5, §6, §9.1, §9.7,
§9.8, §9.9, §9.10, D-2, D-5, D-9; `REQ-P-HOOK-003` §23.1,
`REQ-P-HOOK-004` §11.5, `REQ-P-CRON-001` §24.1, `REQ-P-CRON-002` §24.2,
`REQ-P-USER-DOCS-001`; ADR-068 D3, D5, D6, ADR-069 D1, D2, ADR-066.

## Scope boundary

This sprint owns the user-facing surface. That is the two CLI commands,
the `TIMEOUT_MS` read, the exit-code arms, the doctor collection and
rendering, the CLI surface baseline, the user documentation, and the
composition of `atm-cron` into the daemon. It creates no storage,
protocol, runner, filter, route or scheduler code. It edits no file under
`crates/atm-cron` or `crates/atm-core`. `crates/atm/Cargo.toml`
`[dependencies]` is unchanged.

Owned paths (new unless marked edited):

- `crates/atm/src/commands/hook.rs`, `crates/atm/src/commands/cron.rs`,
  `crates/atm/src/commands/hook_module.rs`, `crates/atm/src/commands/mod.rs`
  (edited: `Hook(HookCommand)`, `Cron(CronCommand)` in `enum Command` at
  `:118` and their arms), `crates/atm/src/output.rs` (edited:
  `print_hook_list`, `print_cron_list`, the hooks block in
  `print_doctor_result` at `:288`, the `HookActions` line),
  `crates/atm/src/main.rs` (edited: `HookModuleInvalid`,
  `HookRefused`, `CronExprInvalid` in the `=> 3` arm of
  `exit_code_for_atm_error`, `:138-151`)
- `crates/atm/tests/cli_surface_baseline.json` (edited),
  `crates/atm/tests/hook_cli.rs`, `crates/atm/tests/cron_cli.rs`,
  `crates/atm/tests/fixtures/hook_module/` (`valid_hook.py`,
  `no_timeout.py`, `zero_timeout.py`, `over_cap.py`, `indented.py`,
  `two_lines.py`, `cron_timeout.py` with `CRON_TIMEOUT_MS = 20000`,
  `cron_over_cap.py` with `CRON_TIMEOUT_MS = 30001`)
- `crates/atm-http-runtime/src/doctor_hooks.rs` (new),
  `crates/atm-http-runtime/src/storage_and_nudge_router.rs` (edited: the
  `doctor()` body at `:651`, one call), `crates/atm-http-runtime/src/lib.rs`
  (edited: mod)
- `crates/atm-daemon-bootstrap/src/lib.rs` (edited: `DaemonWorkers.cron:
  Option<atm_cron::CronWorkerHandle>` at `:532`, the composition in
  `run_replacement_daemon_with_selector` (`:445`), the shutdown step in
  `shutdown_replacement_daemon` (`:805`)),
  `crates/atm-daemon-bootstrap/Cargo.toml` (edited: `atm-cron.workspace =
  true`), `crates/atm-daemon-bootstrap/tests/cron_composition.rs`,
  `crates/atm-daemon-bootstrap/tests/cron_fixture/fixture_cron.py`
- `boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml` and
  `boundaries/atm-daemon-bootstrap/daemon-observability.toml` (edited:
  `atm-cron` in `allowed_dependencies`, the latter at `:25`;
  `cron_worker_lifecycle` in `io_owns` of the former),
  `boundaries/atm-cron/cron-scheduler.toml` (edited: `[status]` notes the
  composition), `.just/lint-config.toml` (edited: `atm-cron` in the
  `crates/atm-daemon-bootstrap/Cargo.toml` allowlist)
- `docs/user-documents/cli-reference.md` (edited: `` ### `atm cron` `` and
  `` ### `atm hook` `` in alphabetical position, a hooks paragraph under
  `` ### `atm doctor` `` at `:109`), `docs/user-documents/tasks.md`
  (edited: "Task hooks"), `docs/user-documents/hooks.md` (new)

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

// crates/atm/src/commands/hook_module.rs  (design.md §9.8 grammar, std only)
pub fn read_timeout_ms(module: &Path) -> Result<u32, AtmError>;   // Err is AtmError::hook_module_invalid naming the module and the rule broken
/// CRON_TIMEOUT_MS by the same grammar, capped at CRON_TIMEOUT_MAX_MS (30 000); absent: read_timeout_ms (Rand 2026-10-18).
pub fn read_cron_timeout_ms(module: &Path) -> Result<u32, AtmError>;
pub fn parse_custom_object(raw: &str) -> Result<serde_json::Map<String, serde_json::Value>, String>;   // "--custom must be a JSON object: <serde error>"

// crates/atm/src/output.rs
pub fn print_hook_list(rows: &[HookRegistration], json: bool) -> Result<()>;   // PHASE HOOK ACTIVE MODULE CWD TIMEOUT_MS CUSTOM REGISTERED_BY REGISTERED_AT DISABLED_AT
pub fn print_cron_list(rows: &[CronSchedule], json: bool) -> Result<()>;       // PHASE NAME ACTIVE CRON JITTER_S MODULE ARGS TIMEOUT_MS CUSTOM STOP LAST_RUN_AT LAST_OUTCOME
// CUSTOM prints `<n> keys`; --json carries the full custom object and jitter_s

// crates/atm-http-runtime/src/doctor_hooks.rs  (design.md §9.10)
pub(crate) async fn collect(router: &StorageAndNudgeRouter, team: Option<&TeamName>, deadline: RequestDeadline) -> Vec<HookDoctorEntry>;
// rows: the team's active hook_registration rows and the active rows of the installed CronScheduler::list(team, None) (none when not installed);
// a schedule becomes a HookRegistration for HookRunner::doctor as bk-4 builds one for a tick; groups by (module_path, cwd, team, phase).
```

Each command does these steps:

- **Preflight.** Every command calls `preflight_daemon_api(..,
  "1.13.0", ..)` (`send.rs:717`) and takes its team from caller context
  (ADR-066).
- **`hook register`.**
  - It reads `read_timeout_ms` and refuses a value above
    `HOOK_TIMEOUT_MAX_MS`.
  - `--cwd` defaults to the module's parent directory. Module and cwd are
    canonicalized.
  - It sends `HookRegister` with `custom` from `--custom` and prints `registered <hook> for
    <team>/<phase> (timeout <n> ms)`.
  - It does not check the hook name. The daemon's `ATM_HOOK_MODULE_INVALID`
    message is printed unchanged.
- **`hook disable`.** It prints one line per row changed. With none it
  prints `no active hooks for phase <p>` and exits 0.
- **`cron start`.**
  - It reads `read_cron_timeout_ms` (`CRON_TIMEOUT_MS`, else
    `TIMEOUT_MS`, at most 30 000) and sends `CronStart` with `stop` taken
    from the clap group and `custom` from `--custom`.
  - It does not check the expression. `ATM_CRON_EXPR_INVALID` is printed
    unchanged.
- **`list --json`.** It prints a bare JSON array, and `[]` when there are
  no rows. Human mode prints `none`. `STOP` is `until <rfc3339 UTC>` or
  `hook`.
- **Task commands.** A `HookActions` response prints `hook ran <n>
  actions` (design D-b). A refused task command prints the hook's message
  and exits 3.

Composition (design §9.9). In `run_replacement_daemon_with_selector`,
after `build_replacement_handler` (`:471`) returns `(handler, recovery,
hook_runner, base_dispatcher)`:

```rust
let cron = Arc::new(atm_cron::AtmCron::new(atm_cron::AtmCronDeps { store: runtime.cron_schedule_store()?, runner: hook_runner.clone(),
    dispatcher: base_dispatcher.clone(), clock: Arc::new(atm_cron::SystemClock), jitter: Arc::new(atm_cron::SystemJitter), host: daemon_atm_config(&env).and_then(|c| c.local_host) }));
handler.install_cron_scheduler(cron.clone())?;
// after start_replacement_runtime_or_abort (:503) returns Ok and before emit_ready_signal_if_requested (:624):
workers.cron = Some(cron.start_worker().await?);   // an initial active_all error aborts startup
// shutdown_replacement_daemon (:805): shutdown_probe::step("cron_worker", ..) is the first step, before "listener"
```

`docs/user-documents/hooks.md` sections, in order:

1. "What a hook is": a filter in front of one `atm task` subcommand with
   four results (handled, pass-through, not-applicable, refused).
2. "The request": the design §2.1 JSON, where `request` is the HTTP body
   of the command, and `custom`, the `--custom` object given at
   registration, with the design §2.1 `/jev-goal` example of one module
   serving many schedules.
3. "The reply": the four design §2.2 shapes, the old `success` key being malformed, actions over any command except the
   hook and cron registry, `as`, the D-c and D-d rules, and what the CLI
   prints.
4. "The module":
   - `TIMEOUT_MS` with its grammar and the 2 500 ms cap, and the optional
     `CRON_TIMEOUT_MS` for cron ticks, capped at 30 000 ms.
   - The design §9.8 `__main__` block.
   - A 40-line example implementing `task.close` and `task.assign` only.
     Every other `task.*` replies `{"result": "not-applicable"}`.
   - `cron(args)`, `stop(args)` and `doctor(request)`, with the design
     §9.10 doctor request and reply JSON and a `doctor` example that
     returns one `warning` finding.
   - A module returns actions rather than calling `atm` for its own hooked
     command. A nested `atm` call is an ordinary request bounded by the
     hook timeout.
5. "Errors you will see": the six codes and their exit codes.
6. "Registering": any roster member may register; any number of phases
   may register the same hook, and each command runs the active phases'
   modules oldest first until one handles; later ones are not run (design
   D-5). A module must answer not-applicable quickly for a task it does not
   own, checking its own bead and task ownership first.
7. "Cron":
   - The grammar of design D-g, with `0 */5 * * * *` as the example, in
     UTC.
   - No replay and one run in flight. Each schedule fires at its
     occurrence plus a fixed random `jitter_s` (design §5 D-r), shown by
     `atm cron list`.
   - The mandatory stop: `--until` or `--until-hook`, with the atm-bd
     phase-bead pattern.
   - `--arg`, and `atm cron end`.
8. "Doctor": one call per module, cwd, team and phase; findings appear
   under `hooks` with their module and phase and never change the
   summary.
9. "Platforms": Windows uses `python` and gets `SystemRoot` and
   `USERPROFILE`. A timeout kills only the child there, while unix kills
   the process group.

## Authoritative deliverables

1. The two commands and the rules above; the CLI validates neither the
   hook name nor the expression (ADR-068 D3, ADR-069 D1).
2. `read_timeout_ms` per design §9.8 (ANC-011).
3. `exit_code_for_atm_error` arms per design §9.1: the three validation
   codes exit 3; `HookExit`, `HookTimeout`, `HookMalformed` exit 1 through
   `_ => 1`.
4. `doctor_hooks::collect` per design §9.10 and the one call in
   `doctor()`; the section never fails doctor and never changes `summary`
   (ANC-015, ANC-016). `print_doctor_result` renders `Hooks:` with one
   line per entry (`<team>/<phase> <module>: <message>` or the error) and
   one indented `<severity> <code>: <message>` line per finding, or
   `Hooks: none`; `--json` carries `hooks`.
5. Composition and shutdown as above (`REQ-P-CRON-002`, ADR-069 D2).
6. `cli_surface_baseline.json` regenerated for the two subtrees only.
7. The user documentation above; `cli-reference.md` lists every flag.
8. Tests, same unit, through the in-process harness of
   `crates/atm/tests/task_ledger_cli.rs` (no live daemon, REQ-BK-TESTS).

## Authoritative acceptance criteria

`crates/atm/tests/hook_cli.rs` unless stated:

- [ ] (ANC-011) `req:REQ-P-HOOK-003` `register_reads_timeout_ms_from_module` (`TIMEOUT_MS = 2500` stored); `register_refuses_bad_timeout_ms_before_any_request`: `no_timeout.py`, `zero_timeout.py`, `over_cap.py` (3000), `indented.py`, `two_lines.py` each exit 3 with `ATM_HOOK_MODULE_INVALID` and the router request counter is 0; `hook_module.rs::tests::timeout_ms_grammar` covers `TIMEOUT_MS: int = 2500  # ms` accepted and `TIMEOUT_MSX = 1` refused.
- [ ] (ANC-013) `req:REQ-P-HOOK-003` `register_refuses_cron_tick_and_unknown_names_with_daemon_message`; `rg -n 'parse_task_hook|TASK_HOOKS' crates/atm/src` is empty.
- [ ] (D-5) `req:REQ-P-HOOK-003` `register_second_phase_same_hook_succeeds`: with phase `bj`'s `task.close` active, `atm hook register task.close --phase bk` exits 0 and `atm hook list` shows both rows active.
- [ ] (ANC-002, ANC-010) `req:REQ-P-HOOK-003` `disable_phase_keeps_rows_and_list_shows_them_inactive`; `disable_with_no_active_hooks_prints_none_and_exits_zero`; `list_json_is_a_bare_array_and_empty_is_brackets`.
- [ ] (ANC-004) `req:REQ-P-HOOK-001` `task_close_refused_prints_hook_message_and_exits_nonzero` (exit 3, task still `active`) and `close_end_to_end_with_refusing_hook_leaves_task_open` through `DaemonHookRunner` and bk-3's `fixture_hook.py`; `hook_actions_response_prints_hook_ran_n_actions`.
- [ ] (REQ-BK-ERRORS) `req:REQ-P-HOOK-003` `crates/atm/src/main.rs::tests::hook_and_cron_codes_map_to_documented_exit_codes`: the six codes give 3, 3, 3, 1, 1, 1 in design §9.1 order.
- [ ] (ANC-015, REQ-P-HOOK-004) `req:REQ-P-HOOK-004` `doctor_hooks_section_dedupes_by_module_cwd_team_and_phase`: two hooks and one schedule on one module in phase `bk` give one entry with both names in `hooks`, the schedule in `schedules`, one runner call, `success == Some(true)` and `message == Some("ok")`; the same module registered in phase `bj` too gives a second entry and a second call; `doctor_findings_carry_module_and_phase` (a reply with one `warning` finding gives an entry with that finding, its `module_path` and `phase`, and `summary` unchanged; a malformed reply gives `error.code == ATM_HOOK_MALFORMED` and doctor exits as without hooks); `doctor_hooks_calls_run_concurrently_under_one_budget` (two `DummyHookRunner` replies released by one `Notify` only after both calls started).
- [ ] (ANC-016) `req:REQ-P-HOOK-004` `doctor_hooks_section_reports_timeout_without_failing_doctor` (exit status and `summary` equal those without registrations); `doctor_hooks_section_is_empty_array_for_team_without_registrations` (`"hooks": []`).
- [ ] (REQ-BK-D3) `req:REQ-P-HOOK-003` `rg -n 'process::Command' crates/atm/src/commands/hook.rs crates/atm/src/commands/cron.rs crates/atm/src/commands/hook_module.rs` is empty: the CLI only sends requests; the composition passes the one `DaemonHookRunner` from `build_replacement_handler` to `AtmCron` (`Arc::ptr_eq` asserted in `cron_scheduler_installed_once_and_cron_routes_reach_it`).
- [ ] (D-2) `req:REQ-P-HOOK-004` `rg -in 'sha|\bbd\b|\.beads' crates/atm/src/commands/hook.rs crates/atm/src/commands/cron.rs crates/atm-http-runtime/src/doctor_hooks.rs` is empty.

`crates/atm/tests/cron_cli.rs`:

- [ ] (ANC-019, D-j) `req:REQ-P-CRON-001` `start_persists_row_with_args_and_timeout`: `--arg status --arg --json --until-hook` stores `args == ["status","--json"]`, `cron_expr` byte-identical, `timeout_ms == 2500`, `stop == Hook`; `list_shows_last_run_and_outcome_after_record_run`; `list_prints_stop_until_in_utc`.
- [ ] (Rand 2026-10-18 cron timeout) `req:REQ-P-CRON-001` `cron_timeout_defaults_to_timeout_ms` (`valid_hook.py` without `CRON_TIMEOUT_MS` sends `timeout_ms == 2500`; `cron_timeout.py` sends `20000`); `cron_start_refuses_cron_timeout_above_30s` (`cron_over_cap.py` exits 3 with `ATM_HOOK_MODULE_INVALID`, request counter 0); `hook register` of `cron_timeout.py` still stores `TIMEOUT_MS` 2500.
- [ ] (ANC-022, Rand 2026-10-18 custom) `req:REQ-P-CRON-001` `custom_flag_reaches_the_row`: `--custom '{"goal":"ship"}'` on `hook register` and `cron start` stores that object, `list --json` carries it and `jitter_s`, and the human table prints `1 keys` and `JITTER_S`; `custom_non_object_refused`: `--custom '[1]'` and `--custom 'x'` are clap errors naming `--custom`, request counter 0.
- [ ] (ANC-023) `req:REQ-P-CRON-001` `start_refuses_missing_stop_condition_before_any_request` and `start_refuses_both_stop_conditions_before_any_request` (clap usage error, counter 0); `start_refuses_until_in_the_past_with_daemon_message`.
- [ ] `req:REQ-P-CRON-001` `start_refuses_invalid_expression_with_cron_expr_invalid` (`x y` and the five-field `* * * * *`, exit 3, list empty); `start_refuses_active_duplicate_name`; `end_phase_keeps_rows_inactive_with_ended_at`.

`crates/atm-daemon-bootstrap/tests/cron_composition.rs`:

- [ ] (ADR-069 D2) `req:REQ-P-CRON-002` `cron_scheduler_installed_once_and_cron_routes_reach_it` (a second `install_cron_scheduler` errors); `cron_worker_starts_after_runtime_and_before_ready_signal`; `cron_worker_is_first_shutdown_step` (probe order begins `cron_worker`, `listener`); `cron_initial_load_error_aborts_startup`.
- [ ] (REQ-BK-D4A) `req:REQ-P-CRON-002` `daemon_restart_with_schedule_due_while_down_fires_once_at_next_occurrence_and_not_at_startup` with `TokioClock` and a paused runtime.
- [ ] (ANC-017, ANC-024) `req:REQ-P-CRON-002` `fixture_cron_receives_args_through_the_composed_scheduler`: `AtmCron` composed as above with `DaemonHookRunner` and `fixture_cron.py`; `run_now` on an `--until-hook` schedule writes the `cron.stop` request and then the `cron.tick` request with `request.args`, each with `sys.argv[1:]` equal to its hook name only, and `last_outcome == "ok"`; after the stop-marker file, `"stopped: marker present"` and `active == false`.

Surface and docs:

- [ ] `req:REQ-P-HOOK-003` `cargo test -p atm --test cli_surface` passes with a baseline diff of only the `hook` and `cron` subtrees; `git diff --stat` shows no change to `crates/atm/Cargo.toml`.
- [ ] `req:REQ-P-USER-DOCS-001` `hooks.md` has the nine sections; its example module defines handlers for `task.close` and `task.assign` only; `cli-reference.md` has `` ### `atm cron` `` and `` ### `atm hook` ``; `just lint` (site-links) passes.

## Authoritative validation

- `cargo test -p atm -p atm-http-runtime -p atm-daemon-bootstrap`
- `cargo clippy -p atm -p atm-http-runtime -p atm-daemon-bootstrap --all-targets -- -D warnings`
- `just lint`
- `just test`

## Non-closure

- No scheduler, runner, filter or route code is written here. The
  live-host walkthrough is the phase-ending gate's evidence.
