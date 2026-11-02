# Sprint bo-15: one PhaseProfile trait for queues, cron, console, show and snapshot

Repo: atm-bd-orchestration. Branch `sprint/bo-15-console-profile-traits`, cut from `integrate/phase-bo`; it consumes nothing from bo-9..bo-12 and develops in parallel with them, then rebases onto the stack top in order. One dev.

Paths below are relative to `plugins/atm-bd-orchestration/` unless they start with `apps/` or `.atm-bd/`. The console app is `apps/atm-bd-console/src/atm_bd_console/` (`<console>` below); its tests are `apps/atm-bd-console/tests/`.

Rand, 2026-11-01 (ANC-075, proposed): "I would expect all the cli subcommands and tui would be working to traits that both skills would need to implement".

## What the subcommands call today (atm-bd-orchestration `origin/main` 1b5d219e)

| Subcommand | Calls, in `<console>/cli.py` | Profile-specific? |
|---|---|---|
| `queues` | `queues.main(args, repo_root=)` → `queues.build` (runs `bd list`, `bd ready`, `atm task list`) → `render` or `cron` (the `--json` diff against a state file); labels from the module constant `QUEUES` | `build` and the labels |
| `show` | `load_snapshot(repo_root, "atm-phase-<x>")` → `render.dashboard` / `render.status_bar` | no: reads `snapshot.json` in the console's `Snapshot` schema |
| `snapshot` | `load_config` → `atm_bd.repo_config` + `which bd/atm/git` → `acquire_lock` → `runner.step(config(dry_run), state, md_table, store)` | the preflight and the step |
| `cron` | the same as `snapshot` without forced dry run, then prints `result.reported`, the GitHub quota line and the errors as JSON lines; `--md-table` prints `status_table.markdown` | the preflight and the step |
| `console` | `load_config` → preflight → `app.run(config, None, INITIAL_STATE, _notification_startup_error(config), runner_step=runner.step)`; the TUI calls `runner_step(config, state, force, store, closed)` each tick and renders the `StepResult`'s `Snapshot` | the preflight, the startup check and the step |

`runner.step` reads beads with `bd`, runs `model.tick`, and dispatches (`atm task assign`, `atm send`) unless `config.dry_run`. That dispatch is orchestration's only. `load_config`, the lock, the `Snapshot` codec, `render`, `status_table` and the TUI widgets are profile-neutral.

## The trait

`<console>/profile.py` defines one Protocol, `PhaseProfile`, with the five calls above that differ by profile:

```python
class PhaseProfile(Protocol):
    def preflight(self, config: Config) -> str | None: ...          # None, or the exact line cli prints before exit 3
    def startup_error(self, config: Config) -> Error | None: ...     # shown in the TUI header
    def queue_report(self, args: argparse.Namespace, repo_root: Path, now: datetime | None) -> dict[str, Any]: ...
    def step(self, config: Config, state: State, force_github: bool, store: StateStore,
             closed: ClosedBeads | None = None) -> StepResult: ...    # console tick and snapshot; writes snapshot.json
    def cron(self, config: Config, state: State, force_github: bool, store: StateStore) -> StepResult: ...
```

and `select(repo_root: Path, phase: str) -> Result[PhaseProfile]`, the one place a profile is chosen.

The Protocol lives in the console, not in `core/atm_bd_core/`: every method takes or returns console types (`Config`, `State`, `StateStore`, `StepResult`, `Snapshot`). Putting it in core would make core import the console, or move those types out of orchestration code into core, which ANC-036 forbids. `core/atm_bd_core/` stays the shared location for profile-neutral code the lite implementation calls (deliverable 6). See Questions.

`show` needs no method: both profiles' `step` write `snapshot.json` through the same `StateStore`, and `show` renders it.

Dispatch is not a trait method. It is a capability inside orchestration's `runner.step`. The lite implementation has no dispatch code, so `console` on a lite phase cannot dispatch (ANC-024). The lite cron's QA assign (ANC-074) is reached only through `cron`, never through `step`.

## Implementations

- **Orchestration: `<console>/orchestration_profile.py`, `OrchestrationProfile`.** It wraps today's code unchanged:
  - `queue_report` = `queues.build(...) | {"kinds": queues.QUEUES}`;
  - `step` and `cron` = `runner.step` with today's arguments;
  - `preflight` = today's `atm_bd.repo_config` check and `which bd/atm/git` loop with today's message text;
  - `startup_error` = `cli._notification_startup_error`.

  Every function the existing tests monkeypatch (`runner.step`, `atm_bd.repo_config`, `atm_bd.phase_file`, `shutil.which`, `config_source.load_config`, `cli._notification_startup_error`, `cli.load_snapshot`, `cli.acquire_lock`, `cli.now`) is looked up at call time through the same module attribute, so those patches still take effect. `cli.load_snapshot` and `cli.acquire_lock` stay in `cli.py`. Nothing under `skills/atm-bd-orchestration/` changes (ANC-030): the console app is not in that skill.
- **Out of process: `<console>/script_profile.py`, `ScriptProfile(profile)`.** Profile-neutral; it never names a profile. It runs `<repo>/.claude/skills/atm-bd-<profile>/scripts/<name>` (or `.codex/skills/...` when that exists instead) through `command.run`, with the phase as `--phase <x>`, and decodes stdout:
  - `queue_report` runs `queues` (passing `--all` and `--stack` when given) and returns its JSON object;
  - `step` runs `snapshot`, decodes `PhaseView`, builds the `Snapshot` with `profile_view.snapshot` (deliverable 3), writes it with `store.write_snapshot` and returns `StepResult(snapshot, state)`; it never runs `cron` and writes no `state.json`;
  - `cron` runs `cron` (plus `--dry-run` when set), decodes `CronOutput`, and returns `StepResult(snapshot, state, reported=<its actions as Action>)`; it writes no file;
  - `preflight` returns a line when a script is missing (fix: install the profile's skill with `atm-bd skill`) or `atm`, `git` or `uv` is not on PATH; `startup_error` returns None.
  - A script exit outside 0 becomes the step's error (`CommandFailed` with the script's stderr), shown like any other tick error.
- **Lite: `skills/atm-bd-lite/scripts/queues`, `snapshot`** (this sprint) **and `cron`** (bo-14). uv scripts with no extension, like orchestration's `bead-queues`. Each reads `.atm-bd/phase-<x>.toml`, refuses with `PROFILE_MISMATCH` and exit 3 unless `profile = "lite"`, reads beads only through `bd_cache_rs.Cache` (ANC-063) and tasks with `atm task list --all --json --as <lead>`, and prints one JSON object. Everything that is not the profile check, argv and the lite queue labels is in `core/atm_bd_core/` (ANC-040).

**Selection** (`profile.select`): read the `profile` key of `.atm-bd/phase-<x>.toml` with `tomllib`. No file, a file that does not parse, no `profile` key, or `profile = "orchestration"` selects `OrchestrationProfile`, which then reports any phase-file error exactly as today. Any other value selects `ScriptProfile(<value>)`. `"orchestration"` is the only profile literal in the console and it appears only in `profile.py`; the console never names `lite`. `queues --root <id>` selects by the phase suffix of the root id.

## Contract types (`<console>/profile_view.py`)

Decoded with the existing `codec.from_json`; frozen dataclasses:
- `ViewBead(id, title, stage, status, assignee, parent, ready, blocked_by, branch, layer, updated_at, closed_at)`.
- `ViewTask(task_id, assignee, state, reminder_count, assigned_at)`.
- `ViewQueueRow(queue, id, stage, status, assignee, since, detail)`.
- `PhaseView(schema, phase, root_id, root_closed, lead, generated_at, beads, tasks, kinds, queues)`; `kinds` is the `(key, label, next actor)` list, `queues` maps a key to its rows.
- `ViewAction(at, kind, to, bead, text, outcome)`; `kind` is `assign` or `send`.
- `CronOutput(view: PhaseView, actions: tuple[ViewAction, ...])`.

`profile_view.snapshot(view, config) -> Snapshot` builds the dashboard from a `PhaseView`: sprints from `stage:sprint` beads and their dev, qa and fix children; one `Section` per non-empty queue except `in_flight` and `not_ready`; `phase_data` as a `Phase` of those sprints and steps; the mermaid graph from sprint `blocked_by` edges; `gh` mode `off`. Fields with no lite fact are None, False, `""` or `()`. The TUI's sanity and quality tabs get `None` (not read) and show their placeholders.

The JSON contract fixtures are `apps/atm-bd-console/tests/fixtures/profile/view-lite.json` (a `PhaseView`) and `cron-lite.json` (a `CronOutput` of a `--dry-run` tick on the same fixture phase). The lite `snapshot` test (this sprint) and the lite `cron` test (bo-14) assert their output equals those files; the console tests decode them. A change on either side fails one of them.

## Deliverables

1. **`PhaseProfile` and `select`** (ANC-075, ANC-005, ANC-044, ANC-061)
   - `<console>/profile.py` as above.
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q tests/test_profile_traits.py -k select` passes. Cases: no phase file, no `profile` key and `profile = "orchestration"` give `OrchestrationProfile`; `profile = "lite"` gives `ScriptProfile` whose script paths are under `.claude/skills/atm-bd-lite/scripts/`; a TOML parse error gives `OrchestrationProfile`.
   - Acceptance: `uv run pyright` (strict, the console's config) reports 0 errors on a test module that assigns `OrchestrationProfile()` and `ScriptProfile("lite")` to `PhaseProfile` variables.
   - Acceptance: in `<console>`, `grep -nwiE 'lite' cli.py queues.py profile.py orchestration_profile.py script_profile.py profile_view.py` prints nothing, and `grep -nw '"orchestration"' cli.py queues.py orchestration_profile.py script_profile.py profile_view.py` prints nothing (only `profile.py` names it; `skill.py`, the installer front end, is out of this check).
   - Acceptance: `git diff --name-only <phase base>..HEAD -- crates/atm-bd-types` prints nothing (ANC-061: no profile names in types).

2. **`OrchestrationProfile`, behavior identical** (ANC-030, ANC-039)
   - `<console>/orchestration_profile.py` as above. `queues.render` and `queues.cron` read labels from `report["kinds"]` instead of the module constant; for an orchestration report that list is `QUEUES`, so their output is unchanged.
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q` passes with every pre-sprint test file unchanged: `git diff --diff-filter=MD --name-only <phase base>..HEAD -- apps/atm-bd-console/tests` prints nothing.
   - Acceptance: `uv run --frozen pytest -q tests/test_cli_help_unchanged.py` passes: the `--help` text of `atm-bd`, `queues`, `console`, `cron`, `show` and `snapshot` equals the text captured at the phase base, committed in `tests/fixtures/help/` with that base's hash in the file header.
   - Acceptance: `git diff --name-only <phase base>..HEAD -- plugins/atm-bd-orchestration/skills/atm-bd-orchestration/` prints nothing.

3. **`ScriptProfile` and the contract types** (ANC-032, ANC-040, ANC-044, ANC-075)
   - `<console>/script_profile.py` and `<console>/profile_view.py` as above. `ScriptProfile` takes a `runner` parameter (default `command.run`) and a `now` parameter.
   - Acceptance: `uv run --frozen pytest -q tests/test_profile_traits.py -k script` passes on a fake runner. It asserts the exact argv for each method (`<skill>/scripts/queues --phase x --all`, `.../snapshot --phase x`, `.../cron --phase x --dry-run`), that `step` writes `snapshot.json` and no `state.json`, that `cron` writes nothing, and that a script exiting 3 becomes one `CommandFailed` tick error carrying the script's stderr line.
   - Acceptance: `uv run --frozen pytest -q tests/test_profile_view.py` passes: `cron-lite.json` decodes to a `CronOutput` whose actions map to `Action`s with `kind`, `to`, `bead_ids` and `outcome` preserved; `view-lite.json` decodes to a `PhaseView`, and `profile_view.snapshot` gives a `Snapshot` whose sprints, steps and sections match the fixture's beads and queues, and whose `phase_data` is not None.

4. **`cli.py` routes every subcommand through the trait** (ANC-005, ANC-024, ANC-027, ANC-075)
   - `queues`: `queues.main(args, repo_root=, build=profile.queue_report)`; `queues.main` gains a keyword `build` that defaults to `queues.build`.
   - `snapshot`, `cron --md-table`: `profile.step`. `cron`: `profile.cron`. `console`: `app.run(..., profile.startup_error(config), runner_step=profile.step)`. `preflight` replaces the inline `repo_config` and `which` checks. `show` is unchanged.
   - The parser is unchanged: no argument is added or removed. Every subcommand takes the same arguments for both profiles (ANC-005). An option a profile does not use (`--github*`, `--interval`, `--no-fetch` for lite) is accepted, and the console writes one stderr line naming it and the profile (`--github: not used by profile lite`); it is never dropped silently. Acceptance: the parametrized test passes the identical argv to both profiles, and for lite asserts that stderr line for each unused option given.
   - ANC-027: `select` is the cron tick's profile check on the console side; the orchestration tick is unreachable for a phase file naming another profile, and each lite script re-checks `profile` itself.
   - Acceptance: `uv run --frozen pytest -q tests/test_profile_traits.py -k "subcommand"` passes. It is parametrized over the 5 subcommands × 2 profiles with one argv list per subcommand, the same for both profiles. Orchestration runs on the existing `tests/fixtures/beads/repo/` fixture with `runner.step` and the `bd`/`atm` runner faked as `test_cli.py` and `test_queues.py` already do, and asserts `runner.step` / `queues.build` received today's arguments. Lite runs on `tests/fixtures/profile/repo-lite/` (a phase file with `profile = "lite"`) with `ScriptProfile`'s runner faked. Each case asserts its exit code and stdout: `queues` prints the lite labels, `snapshot` then `show` prints a dashboard naming the fixture's sprint and bead ids with exit 0, `cron` prints one JSON line per `CronOutput` action, `console` is deliverable 5.
   - Acceptance (ANC-024): the lite `console` and `snapshot` cases assert the fake runner saw no argv containing `cron`, `task assign` or `send`.

5. **The TUI renders a lite phase** (ANC-075, ANC-024)
   - Acceptance: `uv run --frozen pytest -q tests/test_profile_traits.py -k tui` passes. It runs `ConsoleApp` with `ScriptProfile("lite").step` bound to a fake runner returning `view-lite.json`, under `app.run_test()` as `test_app.py` does, presses `r` once and then `q`. It asserts the status text contains the fixture's sprint ids and the step bead ids, the Dispatch panel lists the fixture's non-quiet queue rows, no tick error is shown, and the runner's argv list is only `snapshot` calls. No sleep and no wall clock: `now` is fixed.

6. **Shared lite-usable code in `core/atm_bd_core/`** (ANC-036, ANC-040, ANC-041, ANC-042, ANC-044, ANC-045, ANC-063)
   - This sprint creates the `atm-bd-core` package (`core/pyproject.toml`, `core/atm_bd_core/`, `core/tests/`), the shared location bo-14 names. All of it is newly written.
   - `phase_rows.py`: `beads(listed, ready) -> tuple[PhaseBead, ...]` decodes the dicts `bd_cache_rs.Cache.list(phase)` and `Cache.ready(phase)` return; `tasks(task_list_json) -> tuple[PhaseTask, ...]` decodes `atm task list --all --json`; `read(cache, phase)` makes exactly those two cache calls.
   - `queue_rows.py`: `classify(beads, tasks, now, stall_reminders) -> dict[str, list[QueueRow]]`, by the schema's stage labels and statuses only: in flight, task open but bead closed, stalled (reminders ≥ the argument), claimed with no task, ready dev / fix / qa with no task, sprint with every child closed, other ready, not ready.
   - `phase_view.py`: `view(phase, lead, beads, tasks, kinds, queues, now) -> dict` gives the `PhaseView` JSON object.
   - Fakes `FakeCache`, `FakeRunner` and `make_phase` live once in `core/tests/conftest.py`.
   - Acceptance: `uv run pytest core/tests -q` passes. Each public function runs on orchestration inputs (a byte copy of atm-core `.atm-bd/phase-bk.toml` and `docs/plans/phase-bk.jsonl`, rows wrapped in the `Cache.list` envelope, source commit in the fixture header) and on lite inputs (`tests/fixtures/phase-lite.toml` plus lite rows). `core/tests/test_inventory.py` asserts every public name in `atm_bd_core` is called by at least one test of each kind.
   - Acceptance: `grep -rnwiE 'lite|orchestration' core/atm_bd_core` prints nothing (ANC-044).
   - Acceptance: `git log --follow --name-status --format= -- core/atm_bd_core/` shows only `A` entries (ANC-036).

7. **Lite `queues` and `snapshot` scripts** (ANC-012, ANC-024, ANC-031, ANC-032, ANC-040, ANC-045, ANC-063)
   - `skills/atm-bd-lite/scripts/queues --phase <x> [--all] [--stack]` prints the queue report: `schema`, `generated_at`, `root`, `integration_branch`, `stack`, `queues`, and `kinds` with the lite labels (QA is "Dev closed -> QA ready; cron assigns", no sanity rows). `--stack` sets `stack` to "stack checks run in atm-bd cron (checks 6 and 9)".
   - `skills/atm-bd-lite/scripts/snapshot --phase <x>` prints the `PhaseView` object.
   - The PEP 723 header pins bd-cache-rs 0.1.0 by per-platform URL (`[tool.uv.sources]`, the bo-9 pin) and names `atm-bd-core` as a path source (installed to `.atm-bd/core/` by bo-14).
   - Neither script assigns, sends or writes a file.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_queues_script.py skills/atm-bd-lite/tests/test_snapshot_script.py` passes with a fake `bd_cache_rs` on `sys.path` and a fake runner. Cases: lite phase file gives exit 0 and the expected JSON (`snapshot` output equals `view-lite.json` byte for byte); a phase file with no `profile` key gives `PROFILE_MISMATCH`, exit 3 and no cache call; the script runs from an unrelated working directory.
   - Acceptance: `python3 -c "import ast,sys; [print(f, n.name) for f in sys.argv[1:] for n in ast.parse(open(f).read()).body if isinstance(n,(ast.FunctionDef,ast.ClassDef)) and n.name!='main']" skills/atm-bd-lite/scripts/queues skills/atm-bd-lite/scripts/snapshot` prints nothing: everything but `main` comes from `atm_bd_core` (ANC-040).
   - Acceptance: `grep -nE 'atm_bd_core\.(phase_rows|queue_rows|phase_view)|from atm_bd_core import' skills/atm-bd-lite/scripts/queues skills/atm-bd-lite/scripts/snapshot` names all three modules (ANC-045).
   - Acceptance: `grep -rnE '"bd"|BdCli|bdc|task assign|"send"' skills/atm-bd-lite/scripts/queues skills/atm-bd-lite/scripts/snapshot core/atm_bd_core` prints nothing (ANC-063, ANC-024).

## Not in bo-15
- The lite `cron` script, its QA assign and checks 1-9 (bo-14; it implements `PhaseProfile.cron` through `ScriptProfile` and prints `CronOutput`).
- The lite installer (bo-14).
- Any change to `skills/atm-bd-orchestration/`, to `runner.py`, `model.py` or `app.py`.

## Validation
- `just lint` and `just test` in atm-bd-orchestration.
- `uv run pyright --project core` and `uv run pyright --project skills/atm-bd-lite` (strict) report 0 errors.

## Questions for Rand
- Q1 (ANC-032): "Each atm-bd subcommand must run the script with the same file name from the skill of the profile set in the phase file" does not fit the trait design. Orchestration has no per-subcommand scripts (its `queues` is in-process `queues.build`; its skill script is `bead-queues`), and ANC-030 forbids adding any. `console` and `snapshot` share one lite script, and `show` runs none. Proposed rewording: "Each atm-bd subcommand calls the PhaseProfile implementation selected by the phase file's profile; a profile implemented outside the console supplies each trait method as a script named after the method (queues, snapshot, cron) in its own skill's scripts/ directory."
- Q2 (shared location): the `PhaseProfile` Protocol is in the console app, because its methods take console types; `core/atm_bd_core/` holds only the profile-neutral functions the lite scripts call. Is that the split you want, or should the Protocol move to core with neutral types of its own (more code, no behavior gain)?
- Q3 (lite cron and `show`): lite cron writes no file (2026-11-01 ruling), so `show` on a lite phase reflects the last `console` or `snapshot` tick, not the last cron tick. Should lite cron also write `snapshot.json`? It would be a dashboard cache, not cron state.
