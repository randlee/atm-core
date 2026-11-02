# Sprint bo-15: collectors, rules and policies; every console subcommand on a lite phase

Repo: atm-bd-orchestration. Branch `sprint/bo-15-collect-decide`, cut from `integrate/phase-bo`. Consumes nothing from bo-9..bo-12; rebases onto the stack top in order. One dev.

Paths are relative to `plugins/atm-bd-orchestration/` unless they start with `core/`, `apps/` or `.atm-bd/`. `<console>` is `apps/atm-bd-console/src/atm_bd_console/`; its tests are `apps/atm-bd-console/tests/`. `<phase base>`: the commit the stack was cut from (PR body).

Design: Rand 2026-11-01, ANC-075 and ANC-076 (proposed). The layer is new code in `core/atm_bd_core/`, nothing moved or copied (ANC-036); lite uses it now; it is designed to orchestration's requirements (read from `<console>/runner.py`, `dispatch.py`, `queues.py`, `model.py` at `main` 8714a372); orchestration cuts over later. Open questions: `questions-bo-15.md`.

## Shape

```
phase file --profile--> route (console/profile.py, the one place)
  orchestration / no profile --> today's code, unchanged
  any other profile ---------> skills/atm-bd-<profile>/policy.toml
                                 |  load + validate (typed error at load)
                                 v
  collect(needs of the policy's rules)  -> PhaseFacts       [I/O: atm, git, beads, provider]
  decide(facts, policy)                 -> Decision          [pure]
  view(facts, decision)                 -> PhaseView         [pure]
  execute(decision.actions)             -> outcomes          [I/O: cron only]
```

## Package `core/atm_bd_core/` (new)

| Module | Contents |
|---|---|
| `facts.py` | `FactKind`; one frozen dataclass per kind; `PhaseFacts(phase, now, phase_file, <optional field per kind>, errors)`. |
| `collect/atm.py` | `TASKS` (`atm task list --all --json --as <lead>`), `TASK_EVENTS` (`atm task events <id> --json`, reminders since last start), `REPORTED_KEYS` (`atm search --type atm-bd-cron-report --as <lead>`). |
| `collect/git.py` | `REMOTE_BRANCHES` (one `git ls-remote --heads origin <layer branches>`), `ANCESTRY` (one `git fetch origin <layer branches>` unless `--no-fetch`, then `git merge-base --is-ancestor <below> <above>` per adjacent layer pair). |
| `collect/beads.py` | `BEADS` (`Cache.list(phase)`, plus `Cache.get` for unlisted open-task ids), `READY` (`Cache.ready(phase)`), `STACK` (the `stage:stack` row: assignee, since, `trunk`, `layers`). |
| `collect/provider.py` | `TOP_CHECKS` (exactly one read per tick: `gh api repos/<o>/<r>/commits/<head>/check-runs`, or `az pipelines runs list --branch <b> --top 1 -o json`), `PR_BASES` (one read per layer PR: `gh api repos/<o>/<r>/pulls/<n> --jq .base.ref`, or `az repos pr show --id <n> --query targetRefName -o tsv`). ANC-060 limits. Provider comes from the installed skill's `provider-commands.toml` (bo-14), default `github`. |
| `collect/__init__.py` | `collect(needs: frozenset[FactKind], ports: Ports, phase_file, now) -> PhaseFacts`. Runs only the readers for `needs`; a failed read gives `None` plus `CollectError(kind, argv, message, fix)`. `Ports` = command runner + beads cache. |
| `decide/rule.py` | `Rule(id, needs, params, decide)`; `params` is a frozen dataclass (the schema); `decide(facts, params) -> tuple[Action, ...]`. Actions `Assign(key, to, bead_id, template, vars)`, `Report(key, queue, bead_id, text)`; `key` dedupes. |
| `decide/queues.py`, `decide/stack.py`, `decide/assign.py` | Plain functions; explicit `RULES` tuple per module; no import-time registration. |
| `decide/policy.py` | `TABLE` (`id -> Rule`) from the `RULES` tuples; `load(text, extra_rules, collectors) -> Result[Policy(profile, rules, notify_template, needs)]`. |
| `decide/run.py` | `decide(facts, policy) -> Decision(actions, rows)`. Rules in file order; a rule missing a needed fact is skipped with one `Report("facts-unavailable:<kind>")`. Reports whose key is in `reported_keys` stay view rows; the rest fold into at most one `Notify(to=<lead>, keys)`. |
| `view.py` | `view(facts, decision) -> PhaseView`. |
| `execute.py` | `execute(actions, runner, lead, dry_run) -> tuple[Outcome, ...]`. `Assign` runs `atm task assign <to> --as <lead> --task-id <bead> --template <t> --vars <json>`; `Notify`: `atm send <lead> --as <lead> --template <notify_template> --vars -` (keys as JSON on stdin). Outcome `sent`, `failed <kind>`, `dry_run`. No retry. |
| `tick.py` | `tick(policy, phase_file, ports, now, *, execute_actions: bool, dry_run: bool) -> TickResult(facts, decision, view, outcomes)` |

Public functions return `Result` (ANC-016); collectors decode rows into `facts.py` types, no dict/`Any` (ANC-062); no module names a profile (ANC-044).

### Beads and packaging (ANC-063, ANC-018, ANC-039)
Beads are read in-process through `bd_cache_rs.Cache(<repo>, scopes=[<phase>])` `list`/`ready`/`get` (in the `prerelease/v0.1.0` wheel); no `bd` or `bdc` process (`bdc` has no released binary). bd-cache-rs is not on PyPI, so neither `atm-bd` nor `atm_bd_core` declares it. Lite-only code is imported only on the lite path: `cli.py` imports `policy_route` in the `PolicyRoute` branch, only `policy_route`/`policy_view` import `atm_bd_core`, and only the beads read imports `bd_cache_rs`. A failed import is `CACHE_UNAVAILABLE` with the fix `uv tool install --force atm-bd --with <wheel URL>` (URLs: one constant). `atm_bd_core` ships inside the `atm-bd` wheel (hatch `force-include`); `core/pyproject.toml` is for tests and pyright only.

## Policies

A policy is data: `skills/<skill>/policy.toml`, rule ids in order with params. This sprint adds the lite one:

```toml
profile = "lite"
notify_template = "templates/cron-report.md.j2"
[[rules]]
id = "queue.task_stalled"
params = { stall_reminders = 3 }
# ... every lite rule below
[[rules]]
id = "qa.assign_ready"
params = { member_key = "qa_member", template_key = "qa" }
```

`skills/atm-bd-lite/templates/cron-report.md.j2` (front matter `type: atm-bd-cron-report`, variable `keys`) lists new keys verbatim for `REPORTED_KEYS`; cron keeps no state file.

A skill-only rule lives in `skills/<skill>/rules.py` (`RULES`), named by `[skill_rules] module = "rules.py"`; the console loads it with `skill_loader.load_skill` and passes it as `extra_rules`. Lite has none today.

`policy.load` validates before any read: unknown id (`POLICY_UNKNOWN_RULE`), a needed kind with no collector (`POLICY_NO_COLLECTOR`), params not decoding into the rule's dataclass (`POLICY_BAD_PARAMS`, field and type), `profile` not the phase file's (`PROFILE_MISMATCH`, ANC-027). Each names the rule id; exit 3, never mid-tick.

Extending: a design = a policy file; a decision = a rule + a fixed-facts test; a fact = a `FactKind` + a collector field.

### Lite rule set (built in this sprint)

| Rule id | needs | Action | Orchestration source |
|---|---|---|---|
| `queue.in_flight` | TASKS, BEADS | view row only (quiet) | `queues.py` in_flight |
| `queue.task_bead_closed` (check 1) | TASKS, BEADS | Report `task-bead-closed:<id>` | task_bead_closed |
| `queue.task_stalled` (check 2) | TASKS, TASK_EVENTS | Report `stalled:<id>`; param `stall_reminders` | task_stalled, `STALL_REMINDERS` |
| `queue.claimed_no_task` (check 3) | BEADS, TASKS | Report `claimed-no-task:<id>` | claimed_no_task |
| `queue.ready_work_no_task` (check 4) | BEADS, READY, TASKS | Report `ready-no-task:<id>` (dev/fix) | needs_dev, needs_fix |
| `queue.container_closable` (check 5) | BEADS | Report `closable:<id>` | container_closable |
| `queue.not_ready`, `queue.ready_other` | BEADS, READY | view rows only (quiet) | not_ready, ready_other |
| `stack.ancestry` (check 6) | STACK, ANCESTRY | Report `not-ancestor:<below>:<above>` | `_local_issues` |
| `stack.lock_held` (check 7) | STACK | Report `stack-lock:<assignee>:<since>`; param `max_minutes` | - |
| `stack.top_ci_red` (check 8) | STACK, TOP_CHECKS | Report `ci-red:<head>` | `model._problems` top_ci |
| `stack.layer_issues` (check 9) | STACK, REMOTE_BRANCHES, PR_BASES, REPORTED_KEYS | Report `missing-branch:<b>`, `pr-base:<n>:<base>`, `above-red:<b>` | `_view_issues`, merge `HEAD_NOT_PUSHED` |
| `qa.assign_ready` (ANC-074) | READY, TASKS | Assign `assign:<id>` per unassigned `stage:qa` bead in READY with no open task; member/template from phase-file keys named by params; vars `{task_id, phase, profile}`. Missing key: Report `config:<key>` with fix `add <key> = "..." to .atm-bd/phase-<x>.toml` | `dispatch.candidates` (QA half) |

Lite policy = these rules; no sanity rule (ANC-076).

### Orchestration rules the cutover adds (not built)
Lite list plus `sanity.dispatch` (incl. reopened sanity after FAIL), `queue.needs_sanity`, `queue.needs_dev_fix`, `queue.needs_plan_review`, `queue.finding_fix`, `queue.minor_backlog`, `dispatch.gates` (origin branch, worktree, merge), roster recipient fallback, `pr.missing`, `merge.conflict`, `collision`, `state.unknown`, `github.unavailable`, `phase.stopped`. New facts: `WORKTREES`, `MERGES`, `MEMBERS`, `OPEN_PRS`, `STACK_MEMBERSHIP`. Batched 10-minute notices need `state.json` cycle state: a cutover decision.

## View model and the TUI
`PhaseView`: `phase, root_id, root_closed, lead, generated_at, sprints: tuple[ViewSprint(id, title, status, depends_on, steps: tuple[ViewStep(bead_id, stage, status, assignee, ready)])], tasks: tuple[ViewTask(task_id, assignee, state, reminders, since)], rows: tuple[ViewRow(queue, key, bead_id, text, since)], actions: tuple[ViewAction(kind, to, bead_id, key, outcome)], top_ci: str | None, errors`.

`<console>/policy_view.py: snapshot(view, config) -> Snapshot` maps it onto today's `Snapshot`; `render`, `status_table`, `app.py` unchanged:

| `Snapshot` field | from |
|---|---|
| `phase_data` | `Phase(root_id, label, title, closed, sprints, steps, ())`; stage -> `StepKind` |
| `sprints`, `mermaid` | `SprintView`/`StepView` per `ViewSprint`; edges from `depends_on` |
| `sections` | `Section` per non-quiet queue; `SectionRow(bead_id, age_s, text, None)` |
| `problems` | each non-quiet `ViewRow` as `Problem(key, bead_id, lead, text, since)` |
| `sent` | `actions` as `Action`; outcome `planned` on `console`/`snapshot` |
| `agents`, `task_data` | `AgentView` per assignee with an open task; `Task` per `ViewTask` |
| `top_ci`, `gh` | `top_ci`; `gh` mode `off` when `TOP_CHECKS` was not collected |
| `errors` | `CollectError`s as console `Error`s |
| everything else (incl. sanity/quality tabs) | `None`, `False`, `""`, `()` |

## Console routing (ANC-005, ANC-024, ANC-027, ANC-030, ANC-039, ANC-075)
- `<console>/profile.py: route(repo_root, phase) -> Today | PolicyRoute(profile)`. No file, parse error, no `profile` key or `"orchestration"` -> `Today` (today's code, today's errors); any other value -> `PolicyRoute`. `"orchestration"` appears only here; the console never names `lite`. `queues --root` routes by the id's phase suffix.
- `<console>/monitor.py: PhaseMonitor` Protocol with `queues(args)`, `snapshot(args)`, `step(...)` (the console tick, `runner.step`'s signature) and `cron(args)`; `show` needs no method (it renders `snapshot.json`). Both profiles implement it: `TodayMonitor` delegates to today's code unchanged, `PolicyMonitor` runs the new layer. `cli.py` calls `route` once, then only `PhaseMonitor` methods; the parser is unchanged.
- `<console>/policy_route.py` runs the five subcommands for a `PolicyRoute`. Not `load_config` (it requires orchestration templates); `policy_route.config` builds `Config` from the phase file's `lead`, `root`, `integration_branch` and `console.toml` `interval`.

| Subcommand | Lite path | Writes | Executes actions |
|---|---|---|---|
| `queues` | `tick(execute_actions=False)`; rows by queue in `queues.render` layout; `--json`: non-quiet rows as one object | nothing | no |
| `snapshot` | `tick(execute_actions=False)` under `acquire_lock`; `store.write_snapshot(policy_view.snapshot(...))` | `snapshot.json` | no |
| `show` | unchanged: renders `snapshot.json` | nothing | no |
| `console` | `app.run(config, None, INITIAL_STATE, None, runner_step=policy_route.step)`; `step` has `runner.step`'s signature, `execute_actions=False`, state unchanged | `snapshot.json` | no (ANC-024) |
| `cron` | `tick(execute_actions=True, dry_run=--dry-run)` under `acquire_lock`; one line per outcome `{at, kind, to, bead, text, outcome}`, one `error` line per `CollectError`; `--md-table` prints `status_table.markdown`, executes nothing | nothing (questions Q3) | yes: `Assign`, `Notify` |

Same arguments for both profiles (ANC-005). Lite honours `--phase`, `--dry-run`, `--md-table`, `--interval`, `--no-fetch`, `--github off`, `--all`; any other option given prints one stderr line `<option>: not used by profile <profile>`.

## Deliverables

1. **Facts and collectors** (ANC-076, ANC-063, ANC-060, ANC-040, ANC-041, ANC-042, ANC-016)
   - `core/atm_bd_core/facts.py`, `collect/{__init__,atm,git,beads,provider}.py` as above.
   - Fakes once in `core/tests/conftest.py`: `FakeRunner`, `FakeCache`, `make_phase_file` (ANC-004).
   - Fixtures: orchestration = byte copies of atm-core `.atm-bd/phase-bk.toml` and `docs/plans/phase-bk.jsonl` at `develop` 5debb6db0 (in `fixtures/orchestration/SOURCE`), in the `Cache.list` envelope; lite = `fixtures/lite/phase-lite.toml` plus rows with a three-layer stack bead.
   - Acceptance: `uv run --project core pytest -q core/tests/test_collect.py` passes. Per collector, both fixture sets: decoded facts, exact argv, exactly one check-runs argv per tick, no fetch argv with `no_fetch=True`, `collect({READY})` makes one `ready` call and no argv, a failed read gives `None` + one `CollectError`, and without `bd_cache_rs` the beads collector returns `CACHE_UNAVAILABLE` whose fix names `uv tool install --force atm-bd --with`.
   - Acceptance: `grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch|BdCli|"bd"|"bdc"' core/atm_bd_core` prints nothing.

2. **Rules, policy loading, decide** (ANC-076, ANC-074, ANC-040, ANC-041, ANC-042, ANC-044, ANC-016)
   - `decide/{rule,queues,stack,assign,policy,run}.py`; `skills/atm-bd-lite/policy.toml`; `skills/atm-bd-lite/templates/cron-report.md.j2`.
   - Acceptance: `uv run --project core pytest -q core/tests/test_decide.py` passes. Each lite rule has a firing and a quiet case on a fixed `PhaseFacts` value (no runner, cache or clock fake; fixed `now`), and also runs on the orchestration fixture facts. Among them: `qa.assign_ready` on a READY listing with one unassigned and one assigned `stage:qa` bead gives exactly one `Assign` for the unassigned bead with vars `{task_id, phase, profile}`; `stack.top_ci_red` with the key already in `reported_keys` gives a view row and no `Notify`; a tick with one new key gives exactly one `Notify`; a rule whose needed fact is `None` gives one `facts-unavailable:<kind>` report and is not called.
   - Acceptance: `uv run --project core pytest -q core/tests/test_policy.py` passes: the lite `policy.toml` loads; one invalid fixture per check (`unknown-rule`, `no-collector`, `bad-params`, `profile-mismatch`) gives its typed error naming the rule id; a fixture skill dir with `rules.py` exporting `RULES` loads through `extra_rules`; `import atm_bd_core.decide.queues` twice leaves `TABLE` unchanged (no import-time registration).
   - Acceptance: `grep -rnE 'subprocess|bd_cache_rs|open\(|requests|http' core/atm_bd_core/decide core/atm_bd_core/view.py` prints nothing (ANC-076).
   - Acceptance: `grep -rnwiE 'lite|orchestration|sanity' core/atm_bd_core` prints nothing (ANC-044; the orchestration rules are not built).

3. **View, executor, tick** (ANC-076, ANC-074, ANC-024)
   - `view.py`, `execute.py`, `tick.py` as above.
   - Acceptance: `uv run --project core pytest -q core/tests/test_execute.py core/tests/test_tick.py` passes on `FakeRunner`/`FakeCache`: `execute`: exact assign/send argv and stdin; outcomes `sent`, `failed <kind>`, `dry_run` (no argv); `tick(execute_actions=False)` records no `atm task assign` and no `atm send` argv; `tick` on each fixture set gives a `PhaseView` equal to `fixtures/<set>/view.json`.
   - Acceptance: `grep -rln 'subprocess\|runner(' core/atm_bd_core | sort` prints only `core/atm_bd_core/collect/*.py` and `core/atm_bd_core/execute.py`.

4. **Shared-location proof** (ANC-036, ANC-041, ANC-042, ANC-045)
   - `core/tests/test_inventory.py` asserts every public name in `atm_bd_core` (`__all__`) is called by a test on each fixture set.
   - Acceptance: `uv run --project core pytest -q core/tests` passes.
   - Acceptance: `git log --follow --name-status --format= -- core/atm_bd_core/` shows only `A` entries.
   - Acceptance (ANC-004): `uvx pylint --disable=all --enable=duplicate-code --min-similarity-lines=5 core/tests apps/atm-bd-console/tests/test_policy_route.py` reports no duplicate block.
   - Acceptance (ANC-045): `uv run --frozen pytest -q tests/test_policy_route.py -k modules` (console) passes: running `cron` on the lite fixture repo imports every module under `atm_bd_core` (checked against `sys.modules`).

5. **Routing and the five subcommands on a lite phase** (ANC-075, ANC-005, ANC-024, ANC-027)
   - `<console>/profile.py`, `<console>/policy_route.py`, `<console>/policy_view.py`; `cli.py` calls `route` once.
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q tests/test_policy_route.py` passes. Cases:
     - `route`: no phase file, no `profile` key, `profile = "orchestration"` and a TOML parse error give a `TodayMonitor`; `profile = "lite"` gives `PolicyMonitor("lite")`; pyright strict confirms both satisfy `PhaseMonitor`, and `grep -nE 'TodayMonitor|PolicyMonitor|queues\.main|runner\.step' cli.py` prints only the `route` import line (ANC-075).
     - 5 subcommands x lite, on `tests/fixtures/policy/repo-lite/` (lite phase file + installed `policy.toml`), core fakes, fixed `now`, argv identical to `test_cli.py`'s orchestration argv: `queues` exit 0 with lite labels and bead ids; `snapshot` then `show` prints the fixture's sprint and step ids, exit 0; `cron` prints one `assign` line (the unassigned ready QA bead) and one `send` line; `cron --dry-run` the same with `dry_run` and no assign/send argv; `console` is deliverable 6.
     - ANC-024: for `queues`, `snapshot`, `console` and `show`, the runner saw no argv containing `task assign` or `send`.
     - ANC-005: each unused option given prints its one `not used by profile lite` stderr line.
     - ANC-027: a `policy.toml` whose `profile` differs from the phase file's gives `PROFILE_MISMATCH`, exit 3, and no runner or cache call.
   - Acceptance: in `<console>`, `grep -nwi 'lite' cli.py profile.py policy_route.py policy_view.py` prints nothing, and `grep -lnw '"orchestration"' cli.py policy_route.py policy_view.py` prints nothing.

6. **The TUI renders a lite phase** (ANC-075, ANC-024)
   - Acceptance: `uv run --frozen pytest -q tests/test_policy_route.py -k tui` passes. `ConsoleApp` with `policy_route.step` on the lite fixture under `app.run_test()`, `r` then `q`: status text has the sprint and step ids, Dispatch panel the non-quiet rows, the QA assign shows `planned`, no tick error, no assign/send argv. No sleep, no wall clock.

7. **Orchestration unchanged** (ANC-030, ANC-039)
   - Acceptance: `cd apps/atm-bd-console && uv run --frozen pytest -q` passes, and `git diff --diff-filter=MD --name-only <phase base>..HEAD -- apps/atm-bd-console/tests` prints nothing.
   - Acceptance: `uv run --frozen pytest -q tests/test_cli_help_unchanged.py` passes: `--help` of `atm-bd` and its five subcommands equals the `<phase base>` capture in `tests/fixtures/help/` (hash in header).
   - Acceptance: `git diff --name-only <phase base>..HEAD -- skills/atm-bd-orchestration/ <console>/runner.py <console>/model.py <console>/dispatch.py <console>/queues.py <console>/app.py <console>/render.py` prints nothing.
   - Acceptance: `uv run --frozen pytest -q tests/test_policy_route.py -k orchestration_untouched` passes: each subcommand on `tests/fixtures/beads/repo/` calls `runner.step`/`queues.build` with today's arguments, leaving no `atm_bd_core` or `bd_cache_rs` in `sys.modules`.
   - Acceptance: in an environment without bd-cache-rs (`uv run --frozen python -c 'import bd_cache_rs'` exits 1), `cd apps/atm-bd-console && uv run --frozen pytest -q` and `just test`'s plugin and `.github/scripts/tests` suites pass, and `git diff --diff-filter=MD --name-only <phase base>..HEAD -- apps/atm-bd-console/tests tests '.github/scripts/tests'` prints nothing.
   - Acceptance: in that same environment, `atm-bd cron --phase <x>` on the lite fixture repo exits 3 with one `CACHE_UNAVAILABLE` line naming the fix.

8. **Packaging** (ANC-018, ANC-063)
   - `apps/atm-bd-console/pyproject.toml`: hatch `force-include` of `core/atm_bd_core`; no bd-cache-rs dependency.
   - Acceptance: `uv build apps/atm-bd-console && unzip -l dist/atm_bd-*.whl | grep -c 'atm_bd_core/'` prints a nonzero count, and `unzip -p dist/atm_bd-*.whl '*/METADATA' | grep -ci 'bd-cache-rs'` prints `0`.

## Not in bo-15
- Later phase: the orchestration policy, rules and facts, the cutover, and deleting the old console logic. The cutover needs a parity run on real orchestration phase data (new layer vs today's console, same ticks) before that deletion.
- The lite installer and the live run (bo-14).

## Validation
- `just lint` and `just test` in atm-bd-orchestration.
- `uv run pyright --project core` (strict) reports 0 errors.
