# Sprint bo-15: collectors, rules and policies; every console subcommand on a lite phase

Repo: atm-bd-orchestration. Branch `sprint/bo-15-collect-decide` off `integrate/phase-bo`. Consumes bo-9's atm-bd-types 0.2.0 (`Bead`, `Status`, `Layer`, `Stack`, `Sha`, `CacheStore`, `stub_gen`, `scripts/stub_names.py`); rebases onto the stack top. One dev.

Paths: relative to `plugins/atm-bd-orchestration/` except `core/`, `apps/`, `crates/`, `.atm-bd/`. `<types>` is `crates/atm-bd-types/`. `<console>` is `apps/atm-bd-console/src/atm_bd_console/`; `<phase base>`: the stack's cut commit.

Design: Rand 2026-11-01, ANC-075, ANC-076. Types in Rust (Rand: "general beads, atm, git collector types which are fine in rust"), functions new in `core/atm_bd_core/` (ANC-036), used by lite now, built to orchestration's needs (`<console>/runner.py`, `dispatch.py`, `queues.py`, `model.py` at `main` 8714a372). Questions: `questions-bo-15.md`.

## Crate module `<types>src/monitor/` (new, atm-bd-types 0.3.0)

Same abi3 wheel and generated stub (ANC-034); Python defines no types. Times are unix seconds; no dicts.

| Group | Names |
|---|---|
| Facts | `FactKind` (one member per collector kind below); `TaskFact(task_id, assignee, state: TaskState, bead_id)`; `TaskEvents(task_id, reminders_since_start, last_start)`; `RemoteBranch(name, head: Sha)`; `Ancestry(below, above, is_ancestor)`; `StackFact(bead: Bead, stack: Stack, trunk, since)`; `TopChecks(head: Sha, state: CheckState)` (`PASSING`, `FAILING`, `PENDING`, `NONE`); `PrBase(pr, base)`. BEADS and READY are `list[Bead]`, REPORTED_KEYS `list[str]`. |
| Collected | `PhaseFacts(phase, now, phase_file, <one optional field per kind>, errors: list[CollectError])`; `CollectError(kind, argv, message, fix)` |
| Actions | `Assign(key, to, bead_id, template, vars: AssignVars(task_id, phase, profile))`, `Report(key, queue, bead_id, text)`, `Notify(to, keys)`; `Action = Assign \| Report \| Notify` |
| Rules | trait `Rule` (`id`, `needs -> list[FactKind]`, `params -> list[ParamSpec]`, `decide(facts, params: Params) -> list[Action]`); loader descriptors `ParamSpec(name, kind: ParamKind)` (`INT`, `TEXT`), `Params` (`int(name)`, `text(name)`), `RuleEntry(rule_id, params)`; `Policy(profile, rules: list[RuleEntry], notify_template, needs)`; `MonitorError(id: MonitorErrorId, rule_id, message, fix)` (`POLICY_UNKNOWN_RULE`, `POLICY_NO_COLLECTOR`, `POLICY_BAD_PARAMS`, `PROFILE_MISMATCH`, `CACHE_UNAVAILABLE`) |
| Stage | bo-9's `Stage` gains `STACK` (read by `StackFact`); unknown labels decode to `Stage.OTHER`, so orchestration rows need no sanity member (ANC-044, ANC-068). |
| Results | `Decision(actions, rows: list[ViewRow])`; `PhaseView` and its `ViewSprint`, `ViewStep`, `ViewTask`, `ViewRow`, `ViewAction` (fields below); `ActionOutcome(action, kind: OutcomeKind, failure)` (`SENT`, `FAILED`, `DRY_RUN`; not bo-9's task `Outcome`); `TickResult(facts, decision, view, outcomes)` |
| Ports | trait `CommandRunner` (`run(argv, stdin) -> CommandResult(code, stdout, stderr)`); trait `BeadsCache` (`list(phase)`, `ready(phase)`, `get(id)` -> `list[Bead]`/`Bead` or `MonitorError`); `Ports(runner, cache)`. bo-9's `CacheStore` gains `list`/`ready` and implements `BeadsCache` with the same row decoder (ANC-006, ANC-063). |

## Package `core/atm_bd_core/` (new, functions only)

| Module | Contents |
|---|---|
| `collect/atm.py` | `TASKS` (`atm task list --all --json --as <lead>`), `TASK_EVENTS` (`atm task events <id> --json`, reminders since last start), `REPORTED_KEYS` (`atm search --type atm-bd-cron-report --as <lead>`). |
| `collect/git.py` | `REMOTE_BRANCHES` (one `git ls-remote --heads origin <layer branches>`), `ANCESTRY` (one `git fetch origin <layer branches>` unless `--no-fetch`, then `git merge-base --is-ancestor <below> <above>` per adjacent pair). |
| `collect/beads.py` | `BEADS` (`ports.cache.list(phase)`, plus `get` for unlisted open-task ids), `READY` (`ports.cache.ready(phase)`), `STACK` (the `stage:stack` row as `StackFact`). |
| `collect/provider.py` | `TOP_CHECKS` (one read per tick: `gh api repos/<o>/<r>/commits/<head>/check-runs`, or `az pipelines runs list --branch <b> --top 1 -o json`), `PR_BASES` (one read per layer PR: `gh api repos/<o>/<r>/pulls/<n> --jq .base.ref`, or `az repos pr show --id <n> --query targetRefName -o tsv`); ANC-060 limits. Provider from the installed skill's `provider-commands.toml` (bo-14), default `github`. |
| `collect/__init__.py` | `collect(needs: frozenset[FactKind], ports: Ports, phase_file, now) -> PhaseFacts`: only the readers for `needs`; a failed read gives `None` + `CollectError`. |
| `decide/{queues,stack,assign}.py` | `Rule` classes; one `RULES` tuple per module, no import-time registration; action `key` dedupes. |
| `decide/policy.py` | `TABLE` (`id -> Rule`) from the `RULES`; `load(text, extra_rules, collectors) -> Result[Policy]`. |
| `decide/run.py` | `decide(facts, policy) -> Decision`. Rules in file order; a rule missing a needed fact is skipped with one `Report("facts-unavailable:<kind>")`. Reports already in `reported_keys` stay view rows; the rest fold into at most one `Notify(to=<lead>, keys)`. |
| `view.py` | `view(facts, decision) -> PhaseView`. |
| `execute.py` | `execute(actions, runner, lead, dry_run) -> list[ActionOutcome]`. `Assign` runs `atm task assign <to> --as <lead> --task-id <bead> --template <t> --vars <json>`; `Notify`: `atm send <lead> --as <lead> --template <notify_template> --vars -` (keys as JSON on stdin). No retry. |
| `tick.py` | `tick(policy, phase_file, ports, now, *, execute_actions: bool, dry_run: bool) -> TickResult` |

Public functions return `Result` (ANC-016); no dict/`Any` (ANC-062); code uses crate traits, never `CacheStore` (only `policy_route` builds `Ports`); no profile names (ANC-044).

### Beads and packaging (ANC-063, ANC-018, ANC-039)
`CacheStore` imports `bd_cache_rs` at call time; no `bd`/`bdc` process; bd-cache-rs (not on PyPI) is declared by no package. Only `policy_route`/`policy_view` import `atm_bd_core`; `cli.py` imports `policy_route` only for a `PolicyRoute`. `atm_bd_core` ships in the `atm-bd` wheel, which requires `atm-bd-types==0.3.0`; `core/pyproject.toml` is for tests and pyright only.

## Policies

`skills/<skill>/policy.toml`: `profile`, `notify_template = "templates/cron-report.md.j2"`, ordered `[[rules]]` (`id`, `params`, e.g. `{ stall_reminders = 3 }`).

`skills/atm-bd-lite/templates/cron-report.md.j2` (front matter `type: atm-bd-cron-report`, variable `keys`) lists new keys verbatim for `REPORTED_KEYS`; no state file.

A skill-only rule: `skills/<skill>/rules.py` (`RULES`), named by `[skill_rules] module`, loaded by `skill_loader.load_skill` as `extra_rules`. Lite has none.

`policy.load` validates before any read: unknown id (`POLICY_UNKNOWN_RULE`), a needed kind with no collector (`POLICY_NO_COLLECTOR`), params not matching the rule's `ParamSpec`s (`POLICY_BAD_PARAMS`, field and kind), `profile` not the phase file's (`PROFILE_MISMATCH`, ANC-027). Each names the rule id; exit 3, never mid-tick.

### Lite rule set (built here; no sanity rule, ANC-076)

| Rule id | needs | Action |
|---|---|---|
| `queue.in_flight` | TASKS, BEADS | view row only (quiet) |
| `queue.task_bead_closed` (check 1) | TASKS, BEADS | Report `task-bead-closed:<id>` |
| `queue.task_stalled` (check 2) | TASKS, TASK_EVENTS | Report `stalled:<id>`; param `stall_reminders` |
| `queue.claimed_no_task` (check 3) | BEADS, TASKS | Report `claimed-no-task:<id>` |
| `queue.ready_work_no_task` (check 4) | BEADS, READY, TASKS | Report `ready-no-task:<id>` (dev/fix) |
| `queue.container_closable` (check 5) | BEADS | Report `closable:<id>` |
| `queue.not_ready`, `queue.ready_other` | BEADS, READY | view rows only (quiet) |
| `stack.ancestry` (check 6) | STACK, ANCESTRY | Report `not-ancestor:<below>:<above>` |
| `stack.lock_held` (check 7) | STACK | Report `stack-lock:<assignee>:<since>`; param `max_minutes` |
| `stack.top_ci_red` (check 8) | STACK, TOP_CHECKS | Report `ci-red:<head>` |
| `stack.layer_issues` (check 9) | STACK, REMOTE_BRANCHES, PR_BASES, REPORTED_KEYS | Report `missing-branch:<b>`, `pr-base:<n>:<base>`, `above-red:<b>` |
| `qa.assign_ready` (ANC-074) | READY, TASKS | Assign `assign:<id>` per unassigned `stage:qa` READY bead with no open task; member/template from the phase-file keys the params name; vars `{task_id, phase, profile}`. Missing key: Report `config:<key>`, fix `add <key> = "..." to .atm-bd/phase-<x>.toml` |

## View model and the TUI
`PhaseView`: `phase, root_id, root_closed, lead, generated_at, sprints: list[ViewSprint(id, title, status, depends_on, steps: list[ViewStep(bead_id, stage, status, assignee, ready)])], tasks: list[ViewTask(task_id, assignee, state, reminders, since)], rows: list[ViewRow(queue, key, bead_id, text, since)], actions: list[ViewAction(kind, to, bead_id, key, outcome)], top_ci: str | None, errors`.

`<console>/policy_view.py: snapshot(view, config) -> Snapshot` maps it onto today's `Snapshot`: stage -> `StepKind`, non-quiet rows -> `Section`s and `Problem`s, `actions` -> `sent` (`planned` outside cron), `gh` mode `off` without `TOP_CHECKS`, `CollectError`s -> `Error`s; sanity/quality fields empty.

## Console routing (ANC-005, ANC-024, ANC-027, ANC-030, ANC-039, ANC-075)
- `<console>/profile.py: route(repo_root, phase) -> Today | PolicyRoute(profile)`. No file, parse error, no `profile` key or `"orchestration"` -> `Today` (today's code and errors); else `PolicyRoute`. `"orchestration"` appears only here; the console never names `lite`. `queues --root` routes by the id's phase suffix.
- `<console>/monitor.py: PhaseMonitor` Protocol with `queues(args)`, `snapshot(args)`, `step(...)` and `cron(args)`; `show` needs none. `TodayMonitor` runs today's code, `PolicyMonitor` the new layer; `cli.py` calls `route` once, then only `PhaseMonitor` methods.
- `<console>/policy_route.py` runs the subcommands; its `config` (not `load_config`) builds `Config` from the phase file's `lead`, `root`, `integration_branch` and `console.toml` `interval`.

| Subcommand | Lite path |
|---|---|
| `queues` | `tick(execute_actions=False)`; rows in `queues.render` layout; `--json`: non-quiet rows as one object |
| `snapshot` | same, under `acquire_lock`; `store.write_snapshot(policy_view.snapshot(...))` |
| `show` | unchanged (renders `snapshot.json`) |
| `console` | `app.run(config, None, INITIAL_STATE, None, runner_step=policy_route.step)`; `step` has `runner.step`'s signature, `execute_actions=False` (ANC-024) |
| `cron` | `tick(execute_actions=True, dry_run=--dry-run)` under `acquire_lock`; one line per outcome `{at, kind, to, bead, text, outcome}`, one `error` line per `CollectError`; `--md-table` prints `status_table.markdown`, executes nothing. No state file (Q3) |

Same arguments for both profiles (ANC-005); lite honours `--phase`, `--dry-run`, `--md-table`, `--interval`, `--no-fetch`, `--github off`, `--all`; any other prints one stderr line `<option>: not used by profile <profile>`.

## Deliverables

1. **Facts and collectors** (ANC-076, ANC-063, ANC-060, ANC-040, ANC-041, ANC-042, ANC-016)
   - `core/atm_bd_core/collect/{__init__,atm,git,beads,provider}.py` as above, on deliverable 9's types.
   - Fakes once in `core/tests/conftest.py`: `FakeRunner`, `FakeCache`, `make_phase_file` (ANC-004).
   - Fixtures: orchestration = byte copies of atm-core `.atm-bd/phase-bk.toml` and `docs/plans/phase-bk.jsonl` at `develop` 5debb6db0 (`fixtures/orchestration/SOURCE`); lite = `fixtures/lite/phase-lite.toml` plus rows with a three-layer stack bead.
   - Acceptance: `uv run --project core pytest -q core/tests/test_collect.py` passes. Per collector, both fixture sets: decoded facts, exact argv, exactly one check-runs argv per tick, no fetch argv with `no_fetch=True`, `collect({READY})` makes one `ready` call and no argv, a failed read gives `None` + one `CollectError`, and without `bd_cache_rs` `CacheStore.for_phase` gives `CACHE_UNAVAILABLE` whose fix names `uv tool install --force atm-bd --with`.
   - Acceptance: `grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch|BdCli|"bd"|"bdc"' core/atm_bd_core` prints nothing.

2. **Rules, policy loading, decide** (ANC-076, ANC-074, ANC-040, ANC-041, ANC-042, ANC-044, ANC-016)
   - `decide/{queues,stack,assign,policy,run}.py`; `skills/atm-bd-lite/policy.toml`; `skills/atm-bd-lite/templates/cron-report.md.j2`.
   - Acceptance: `uv run --project core pytest -q core/tests/test_decide.py` passes. Each lite rule has a firing and a quiet case on a fixed `PhaseFacts` (no I/O fakes, fixed `now`) and runs on the orchestration fixture facts, including: `qa.assign_ready` on READY with one unassigned and one assigned `stage:qa` bead gives one `Assign`, for the unassigned one, with vars `{task_id, phase, profile}`; `stack.top_ci_red` with the key already in `reported_keys` gives a view row and no `Notify`; a tick with one new key gives exactly one `Notify`; a rule whose needed fact is `None` is not called and gives one `facts-unavailable:<kind>` report.
   - Acceptance: `uv run --project core pytest -q core/tests/test_policy.py` passes: the lite `policy.toml` loads; one invalid fixture per check gives its typed error naming the rule id; a fixture skill `rules.py` loads through `extra_rules`; importing `decide.queues` twice leaves `TABLE` unchanged.
   - Acceptance: `grep -rnE 'subprocess|bd_cache_rs|open\(|requests|http' core/atm_bd_core/decide core/atm_bd_core/view.py` prints nothing (ANC-076).
   - Acceptance: `grep -rnwiE 'lite|orchestration|sanity' core/atm_bd_core` prints nothing (ANC-044).

3. **View, executor, tick** (ANC-076, ANC-074, ANC-024)
   - `view.py`, `execute.py`, `tick.py`.
   - Acceptance: `uv run --project core pytest -q core/tests/test_execute.py core/tests/test_tick.py` passes on the fakes: `execute` gives exact assign/send argv and stdin and outcomes `sent`, `failed <kind>`, `dry_run` (no argv); `tick(execute_actions=False)` records no assign or send argv; `tick` per fixture set gives the `PhaseView` in `fixtures/<set>/view.json`.
   - Acceptance: `grep -rln 'subprocess\|runner\.run(' core/atm_bd_core | sort` prints only `core/atm_bd_core/collect/*.py` and `core/atm_bd_core/execute.py`.

4. **Shared-location proof** (ANC-036, ANC-041, ANC-042, ANC-045)
   - Acceptance: `uv run --project core pytest -q core/tests` passes, including `test_inventory.py`: every `atm_bd_core.__all__` name is called by a test on each fixture set.
   - Acceptance: `git log --follow --name-status --format= -- core/atm_bd_core/` shows only `A` entries.
   - Acceptance (ANC-004): `uvx pylint --disable=all --enable=duplicate-code --min-similarity-lines=5 core/tests apps/atm-bd-console/tests/test_policy_route.py` reports no duplicate block.
   - Acceptance (ANC-045): `uv run --frozen pytest -q tests/test_policy_route.py -k modules` (console) passes: lite `cron` imports every `atm_bd_core` module (`sys.modules`).

5. **Routing and the five subcommands on a lite phase** (ANC-075, ANC-005, ANC-024, ANC-027)
   - `<console>/profile.py`, `<console>/policy_route.py`, `<console>/policy_view.py`; `cli.py` calls `route` once.
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q tests/test_policy_route.py` passes. Cases:
     - `route`: no phase file, no `profile` key, `profile = "orchestration"` and a TOML parse error give a `TodayMonitor`; `profile = "lite"` gives `PolicyMonitor("lite")`; pyright strict: both satisfy `PhaseMonitor`; `grep -nE 'TodayMonitor|PolicyMonitor|queues\.main|runner\.step' cli.py` prints only the `route` import line (ANC-075).
     - 5 subcommands x lite, on `tests/fixtures/policy/repo-lite/` (lite phase file + `policy.toml`), core fakes, fixed `now`, `test_cli.py`'s orchestration argv: `queues` exits 0 with lite labels and bead ids; `snapshot` then `show` prints the sprint and step ids; `cron` prints one `assign` line (the unassigned ready QA bead) and one `send` line; `cron --dry-run` the same with `dry_run` and no assign/send argv.
     - ANC-024: `queues`, `snapshot`, `console`, `show` give no argv containing `task assign` or `send`.
     - ANC-005: each unused option prints its `not used by profile lite` line.
     - ANC-027: a `policy.toml` whose `profile` differs from the phase file's gives `PROFILE_MISMATCH`, exit 3, and no runner or cache call.
   - Acceptance: in `<console>`, `grep -nwi 'lite' cli.py profile.py policy_route.py policy_view.py` and `grep -lnw '"orchestration"' cli.py policy_route.py policy_view.py` print nothing.

6. **The TUI renders a lite phase** (ANC-075, ANC-024)
   - Acceptance: `uv run --frozen pytest -q tests/test_policy_route.py -k tui` passes. `ConsoleApp` with `policy_route.step` on the lite fixture, `app.run_test()`, `r` then `q`: status text has the sprint and step ids, Dispatch panel the non-quiet rows, the QA assign shows `planned`; no tick error, no assign/send argv, no sleep or wall clock.

7. **Orchestration unchanged** (ANC-030, ANC-039)
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q` passes.
   - Acceptance: `uv run --frozen pytest -q tests/test_cli_help_unchanged.py` passes: `--help` of `atm-bd` and its five subcommands equals the `<phase base>` capture in `tests/fixtures/help/`.
   - Acceptance: `git diff --name-only <phase base>..HEAD -- skills/atm-bd-orchestration/ <console>/runner.py <console>/model.py <console>/dispatch.py <console>/queues.py <console>/app.py <console>/render.py` prints nothing.
   - Acceptance: `uv run --frozen pytest -q tests/test_policy_route.py -k orchestration_untouched` passes: each subcommand on `tests/fixtures/beads/repo/` calls `runner.step`/`queues.build` with today's arguments; no `atm_bd_core` or `bd_cache_rs` in `sys.modules`.
   - Acceptance: without bd-cache-rs (`uv run --frozen python -c 'import bd_cache_rs'` exits 1), `just test` passes, `git diff --diff-filter=MD --name-only <phase base>..HEAD -- apps/atm-bd-console/tests tests '.github/scripts/tests'` prints nothing, and `atm-bd cron --phase <x>` on the lite fixture repo exits 3 with one `CACHE_UNAVAILABLE` line naming the fix.

8. **Packaging** (ANC-018, ANC-063)
   - `apps/atm-bd-console/pyproject.toml`: hatch `force-include` of `core/atm_bd_core`; no bd-cache-rs dependency.
   - Acceptance: `uv build apps/atm-bd-console && unzip -l dist/atm_bd-*.whl | grep -c 'atm_bd_core/'` prints a nonzero count, and `unzip -p dist/atm_bd-*.whl '*/METADATA' | grep -ci 'bd-cache-rs'` prints `0`.

9. **Crate module `monitor`, stub, release 0.3.0** (ANC-034, ANC-045, ANC-061, ANC-062, ANC-068, ANC-018)
   - `<types>src/monitor/` with the names in the crate table; `CacheStore` implements `BeadsCache`; `<types>Cargo.toml` and `pyproject.toml` say `0.3.0`; `core/pyrightconfig.json` (strict, `stubPath` `../crates/atm-bd-types`).
   - Acceptance (ANC-034): `cd crates/atm-bd-types && cargo run --features python --bin stub_gen && git diff --exit-code atm_bd_types.pyi` exits 0; `cargo test -p atm-bd-types --features python monitor` passes.
   - Acceptance (ANC-061): `grep -rniwE 'lite|orchestration' crates/atm-bd-types/src crates/atm-bd-types/atm_bd_types.pyi` prints nothing.
   - Acceptance (ANC-068, ANC-045): `python3 crates/atm-bd-types/scripts/stub_names.py crates/atm-bd-types/atm_bd_types.pyi <plan>/hooks/*.md skills/atm-bd-lite/scripts/atm-bd-task-*.py core/atm_bd_core` prints nothing and exits 0 (`<plan>` as in bo-9).
   - Acceptance (ANC-062): `uv run pyright --project core` reports `0 errors` against the stub, and `grep -rn '@dataclass' core/atm_bd_core` prints nothing.
   - Release: `crate-release.yml` `workflow_dispatch`, `version=0.3.0`, before the `atm-bd` release; then `curl -s https://pypi.org/pypi/atm-bd-types/json | jq -r '.releases|keys[]'` lists `0.3.0`.
   - Acceptance (ANC-018): `grep -nE 'crate-release|cargo publish|atm-bd-types' .github/workflows/release.yml` prints nothing: Python-only fixes release with no crate publish.

## Not in bo-15
- Later phase: the orchestration policy (sanity dispatch, other `queues.py` queues, dispatch gates, PR/merge/state notices; facts `WORKTREES`, `MERGES`, `MEMBERS`, `OPEN_PRS`, `STACK_MEMBERSHIP`) and the cutover, after a parity run on real orchestration data.
- Lite installer and live run (bo-14).
- When beads-cache is integrated, the beads collector and later the rules move to Rust behind the same crate traits (`Ports`, `Rule`); the facts and action types, the policies, the console and the fixed-facts tests do not change. Python therefore depends only on crate traits and types, never a concrete collector.

## Validation
- `just lint` in atm-bd-orchestration.
