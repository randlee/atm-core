# Sprint bo-14: lite cron, lite install, one live lite phase

Repo: atm-bd-orchestration. Branch `sprint/bo-14-lite-live`, cut from the top of the phase-bo stack once it holds bo-9..bo-12 and bo-15. One dev; the live run (deliverable 8) is run by one agent (ANC-015).

Paths below are relative to `plugins/atm-bd-orchestration/` unless they start with `apps/` or `.atm-bd/`. The lite skill is `skills/atm-bd-lite/` (ANC-028).

## Dependencies (the artifact each one supplies)
- **bo-9**: `atm-bd-types` 0.2.0. Cron uses `CacheStore.for_phase` and `CacheStore.show` (the bd_cache_rs adapter), plus `Bead`, `Stack`, `Status` and `ErrorId.PROFILE_MISMATCH`.
- **bo-15**: the `PhaseProfile` trait and its out-of-process adapter. `atm-bd cron --phase <x>` on a lite phase file calls `PhaseProfile.cron`, which `ScriptProfile` implements by running `.claude/skills/atm-bd-lite/scripts/cron --phase <x> [--dry-run]` and decoding its stdout as `CronOutput` (`view: PhaseView`, `actions: [ViewAction(at, kind, to, bead, text, outcome)]`, `<console>/profile_view.py`). Cron also uses the `core/` package bo-15 creates and its `atm_bd_core.phase_rows.read(cache, phase)` (the `Cache.list` and `Cache.ready` listing), `phase_view.view(...)`, `queue_rows.classify(...)`, and the fakes in `core/tests/conftest.py`.
- **bo-10**: the three installed hooks `atm-bd-task-assign-hook.py`, `atm-bd-task-start-hook.py` and `atm-bd-task-hook.py`. The live run calls them.
- **bo-11**: the dev template's stack block and the QA template's pour block. Cron also uses the dispatch variable contract `{task_id, phase, profile}` (bo-11 deliverable 3) and the phase-file keys `profile` and `[templates].qa` (bo-11 deliverable 5).
- **bo-12**: `skills/atm-bd-lite/scripts/atm-bd-lite-gate.py` must exit 0 on the three bo-10 hooks at the bo-14 top.
- **bo-13** (atm-core): `log_run` records `{code, message, fix, context}` and the actions of every hook reply. The live run reads those records with `atm log filter`. The atm-core release that carries bo-13 must be installed on the run host.
- **randlee/beads-cache#104**: a beads-cache release with the pour/create op, pinned by bo-9. The pour block in the run's findings round needs it.
- **A released `bdc` binary** (bo-11 Q8, blocked on randlee/sc-publish#135). The stack and pour blocks run `bdc` live, so deliverable 8 cannot start until one is installed on the run host. Deliverables 1-7 and 9 do not need it: cron reads through the released `bd_cache_rs` wheel.

## How cron reads beads
Rand said "reads through bdc". ANC-063 says beads go only through beads-cache (bd_cache_rs). Both name beads-cache, which has two front ends: the `bdc` CLI, whose contract is on beads-cache `develop` but which has no released binary, and the bd_cache_rs library (released wheel, pinned by bo-9). Cron uses the library through bo-15's `phase_rows.read` (`Cache.list`, `Cache.ready`) and bo-9's `CacheStore.show`. It runs no `bd` and no `bdc` subprocess. This satisfies both statements and does not wait on a `bdc` binary release. Its only subprocesses are `atm`, `git` and the provider CLI, and it is not a hook, so ANC-066 does not apply to it.

## Cron is the lite `PhaseProfile.cron` (ANC-075, ANC-032)
- bo-15 routes every subcommand through `PhaseProfile`, selected by the phase file's profile. On a lite phase file `atm-bd cron --phase <x>` runs `skills/atm-bd-lite/scripts/cron` through `ScriptProfile.cron`; on any other phase file it runs orchestration's in-process tick unchanged. bo-14 adds no routing.
- The script is named after the trait method, `cron`, a uv script with no extension like bo-15's `queues` and `snapshot`. ANC-032's wording for this is bo-15 Q1.
- Its stdout is one `CronOutput` JSON object. The console prints each action as today's cron JSON line (`at`, `kind`, `to`, `bead`, `text`, `outcome`), so a lite and an orchestration cron tick print the same line shape.

## Shared location
The shared location is the `atm-bd-core` package at `core/atm_bd_core/`, following Rand's 2026-11-01 note "atm-bd-core is the shared location" (the `atm-bd-shared` package is retired, and bo-1's `core/` held code moved out of orchestration, which ANC-036 forbids). bo-15 creates the package. The lite installer installs it to `.atm-bd/core/`, and each lite script's uv header names it as a path source. bo-14 adds two modules there: `cron_checks.py` and `cron_facts.py`. Both are newly written; nothing is copied from orchestration. This definition needs Rand's confirmation (bo-15 Q2).

## Deliverables

1. **QA assignment in cron** (ANC-074, ANC-040, ANC-045)
   - `atm_bd_core/cron_checks.py::qa_to_assign(ready)` returns the QA beads in a `CacheStore.ready` listing that have no assignee.
   - For each one, `cron` runs `atm task assign <qa_member> --as <lead> --task-id <bead> --template <templates.qa> --vars <json>`.
     - `lead`, `qa_member` and `templates.qa` come from the phase file.
     - `vars` is `{"task_id": <bead>, "phase": <x>, "profile": "lite"}`, the whole bo-11 dispatch contract.
   - Each assign is one `ViewAction` (`kind = "assign"`, its outcome `sent` or `failed <kind>`) in the `CronOutput` the script prints. With `--dry-run` cron runs no assign and reports outcome `dry_run`.
   - If the phase file has no `qa_member`, cron refuses with the fix `add qa_member = "<agent>" to .atm-bd/phase-<x>.toml` and assigns nothing.
   - Cron takes no automatic action other than this assignment.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_cron.py` passes. The fake ready listing has one unassigned and one assigned ready QA bead. The test asserts exactly one assign argv, for the unassigned bead, and one matching `assign` action in the printed `CronOutput`.

2. **Checks 1-9 as pure shared functions** (ANC-040, ANC-044, ANC-016, ANC-062)
   - Each check is one function in `atm_bd_core/cron_checks.py`.
   - Each function takes typed facts and returns a list of `CronItem(key, text)`.
   - The current time and all thresholds are arguments. A function reads no clock, file, network or subprocess.
   - The checks:
     1. ATM task open but bead closed.
     2. Stalled task: 3 or more reminder events since its last start.
     3. Bead claimed (assigned or in_progress) with no open ATM task.
     4. Ready dev or fix bead with no task. Notify only.
     5. Sprint bead with every child closed.
     6. Stack layer ancestry: each layer's head descends from the layer below's head.
     7. Stack-bead lock held too long: the stack bead's assignee has been set longer than the threshold.
     8. CI failing on the top layer's head. The item is keyed `ci-red:<head>`, so each head is reported once.
     9. Stack issues:
        - a layer branch missing on the remote;
        - a PR base that is not the branch of the layer below (the trunk for layer 1);
        - a layer recorded above a layer whose head has a prior `ci-red` report.
   - `new_items(items, prior_keys)` keeps only items whose key was not already reported.
   - Acceptance: `uv run pytest core/tests/test_cron_checks.py` passes. Each check has one firing case and one quiet case built from fixed inputs. Check 7 takes a fixed `now`. No test sleeps, uses threads or reads the clock.

3. **Fact readers** (ANC-040, ANC-060, ANC-063)
   - `atm_bd_core/cron_facts.py` turns command output into the typed facts the checks take. Every reader takes a command-runner parameter.
   - The commands, by fact:

     | Fact | Command |
     |---|---|
     | Open tasks | `atm task list --all --json --as <lead>` |
     | Reminder count | `atm task events <id> --json` |
     | Ancestry | `git merge-base --is-ancestor <below> <above>`, after one `git fetch origin <layer branches>` per tick |
     | Remote branches | one `git ls-remote --heads origin <branches>` |
     | Top CI | exactly one provider check-runs read per tick: `gh api repos/<owner>/<repo>/commits/<head>/check-runs` (GitHub) or `az pipelines runs list --branch <branch> --top 1 -o json` (Azure) |
     | PR base | one provider PR read per layer: `gh api repos/<owner>/<repo>/pulls/<n> --jq .base.ref` or `az repos pr show --id <n> --query targetRefName -o tsv` |
     | Prior reports | `atm search "<key>" --type lite-cron-report --from <lead>` |

   - ANC-060 bans `gh pr list`, `gh pr checks`, `gh run watch` and any `--watch`. `gh api .../check-runs` is not on that list, so check 8 uses it.
   - Beads come only from `CacheStore`.
   - Acceptance: `uv run pytest core/tests/test_cron_facts.py` passes. A fake runner returns recorded outputs for each command. The test asserts the parsed facts and the exact argv list, including exactly one check-runs argv per tick.
   - Acceptance: `grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch|BdCli|"bd"' core/atm_bd_core skills/atm-bd-lite` prints nothing.

4. **The `cron` script, lite implementation of `PhaseProfile.cron`** (ANC-027, ANC-031, ANC-032, ANC-045, ANC-074, ANC-075)
   - `skills/atm-bd-lite/scripts/cron --phase <x> [--dry-run]` reads `.atm-bd/phase-<x>.toml`.
   - If the profile is not `lite`, it prints `PROFILE_MISMATCH` with the fix text and exits 3. It assigns and sends nothing. A phase file with no `profile` key counts as not lite.
   - Otherwise it does three things and prints one `CronOutput` (bo-15) whose `view` comes from bo-15's `phase_view.view`:
     - reads the facts (deliverable 3);
     - assigns QA (deliverable 1);
     - runs checks 1-9 and sends at most one ATM message to the lead, and only when `new_items` is non-empty. The message body is rendered from `skills/atm-bd-lite/templates/lite-cron-report.md.j2` and lists every new key verbatim, so later ticks can find it with `atm search`. The send is one `ViewAction` with `kind = "send"`.
   - No state file is written.
   - `PROFILE = "lite"` appears only in the lite scripts (`cron`, and bo-15's `queues` and `snapshot`). The shared modules hold no profile literal.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_cron.py` passes and covers four cases:
     - an orchestration phase file (no `profile`) gives `PROFILE_MISMATCH`, exit 3, and no argv run;
     - a tick with only already-reported keys sends nothing;
     - a tick with one new item sends exactly one `atm send`;
     - `--dry-run` runs no `atm task assign` and no `atm send`, reports each planned assign with outcome `dry_run`, and its stdout on the bo-15 lite fixture equals bo-15's contract fixture `apps/atm-bd-console/tests/fixtures/profile/cron-lite.json`;
     - the script runs from an unrelated working directory.

5. **Lite installer** (ANC-001, ANC-028, ANC-031, ANC-012)
   - `atm-bd skill --lite [--github | --azure] [--dest REPO] [--codex]` runs `skills/atm-bd-lite/scripts/install.py`. It installs:
     - `skills/atm-bd-lite/` to `.claude/skills/atm-bd-lite/` (or `.codex/`);
     - `core/atm_bd_core/` to `.atm-bd/core/`.
   - The installer renders the chosen provider's command set with sc-compose into `skills/atm-bd-lite/templates/provider-commands.toml.j2`, which cron reads. It also renders every provider placeholder in bo-11's templates; today those templates hard-code `gh pr view`/`gh pr create`/`gh pr edit` and have no placeholder (Questions).
   - The lite install writes nothing under `skills/atm-bd-orchestration/`, and the orchestration install writes nothing under `skills/atm-bd-lite/`.
   - `skills/atm-bd-lite/SKILL.md` (`name: atm-bd-lite`) gets an "Install" section and a "Cron" section. bo-11 owns the "Dispatch" section. Together they cover:
     - install;
     - set `profile = "lite"`, `lead`, `qa_member` and `[templates]` in the phase file (bo-11's `phase-file.example.toml` plus `qa_member`);
     - add the crontab line `atm-bd cron --phase <x>`;
     - assign → start → stack block → close.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_install.py` passes:
     - `--azure` renders `provider-commands.toml` with `az repos pr` and no `gh pr`, and `--github` renders the reverse. The dev template is in the anchor's check too, so this part of ANC-001 waits on the provider-placeholder question;
     - `SKILL.md` names `atm-bd-lite`;
     - after a lite install, `find .claude/skills/atm-bd-orchestration` is empty;
     - an `--orchestration` install into a temp repo gives the same tree, byte for byte, as at the phase base.

6. **Orchestration not broken, not changed** (ANC-030, ANC-039, ANC-036)
   - Acceptance: `git diff --name-only <phase base>..HEAD -- plugins/atm-bd-orchestration/skills/atm-bd-orchestration/` prints nothing.
   - Acceptance: `git diff --diff-filter=MD --name-only <phase base>..HEAD -- plugins/atm-bd-orchestration/tests apps/atm-bd-console/tests '*/tests/test_*'` prints nothing. Tests are only added, never modified or deleted.
   - Acceptance: `just test` passes at the bo-14 top. That run includes orchestration's whole suite: console pytest, `.github/scripts/tests`, the `gen_manifest.py --check` and the plugin pytest. `<phase base>` is the commit the phase-bo stack was cut from, recorded in the PR body.
   - Acceptance: `git log --follow --name-status --format= -- core/atm_bd_core/` shows only `A` entries, no `R`/`C` from a `skills/atm-bd-orchestration/` path.

7. **Shared-code proof on both profiles** (ANC-041, ANC-042, ANC-044, ANC-045, ANC-004)
   - Every public function in `cron_checks.py` and `cron_facts.py` has two test runs:
     - **Orchestration inputs**: a byte copy of atm-core's `.atm-bd/phase-bk.toml` and `docs/plans/phase-bk.jsonl`, with the source commit in the fixture's header file.
     - **Lite inputs**: a lite phase file fixture.
   - Fakes (`FakeBeads`, `FakeRunner`, `make_phase`) live once in `core/tests/conftest.py` and `skills/atm-bd-lite/tests/conftest.py`. No test repeats their setup inline.
   - Acceptance: `uv run pytest core/tests -q` passes, and a test inventory (`core/tests/test_inventory.py`, created by bo-15) asserts that every public name in `atm_bd_core` is called by at least one orchestration-input test and one lite-input test.
   - Acceptance: `grep -rnE "lite|orchestration" core/atm_bd_core` prints nothing.
   - Acceptance: `git diff --name-only <phase base>..HEAD -- core/atm_bd_core` lists only modules imported by a lite script (`cron`, `queues`, `snapshot`).

8. **One live lite phase, end to end** (ANC-015, ANC-074, ANC-018)
   - **Release.** Release atm-bd as a Python-only version carrying bo-9..bo-14, and install it from the release assets (`atm-bd skill --lite --github`). `gh run view <release run> --json jobs --jq '.jobs[].name'` lists no crate publish job (ANC-018).
   - **Setup.** In a phase file with `profile = "lite"`, one ATM identity is `lead` and `qa_member`. A crontab runs `atm-bd cron --phase <x>`. The phase has one sprint whose dev bead is a real open atm-bd-orchestration issue.
   - **Sequence:**
     1. Lead `atm task assign` (dev).
     2. `atm task start`.
     3. Dev work.
     4. The stack block.
     5. `atm task close completed`, which goes through the hook as T-CLOSE-02.
     6. A cron tick assigns the released QA bead (deliverable 1).
     7. QA with findings: the pour block, then close as T-CLOSE-04.
     8. Lead assigns the fix bead (cron check 4 reports it ready with no task).
     9. Fix: start, stack block, close.
     10. A cron tick assigns the fix's QA bead.
     11. QA PASS (T-CLOSE-03).
     12. The stack lands.
   - **One refused close.** At least one close is refused during the run. If none happens naturally, the dev closes once before running the stack block, which gives `STACK_NOT_RECORDED`.
   - **Real findings only.** If the first QA passes with no real finding, the run continues with the next real sprint until a findings round happens. No seeded defect.
   - **Evidence**, pasted verbatim into the bo-14 close report; no evidence files:
     - `atm task events <id> --json` for every task;
     - every cron tick's stdout;
     - `atm log filter` records for every hook invocation in the run;
     - each bead's final state, read through `CacheStore.show`;
     - the landed stack's `git log --oneline <trunk>..<top>`.
   - Acceptance (ANC-015): every assign, start, close and cron-issued assign in the events comes from the one identity.
   - Acceptance (ANC-074): the QA tasks are assigned by cron. The events show the actor `<lead>` at the cron tick's time, and no hand assign of a QA bead appears.
   - Acceptance (bo-13): the `atm log filter` record for the refused close carries `code = STACK_NOT_RECORDED` (or the refusal that actually happened), a non-empty `message` and the `fix` text from `hooks/task-close.md`. Every other hook result of the run also has a record.

9. **No repair scripts** (ANC-059)
    - Acceptance: `find skills/atm-bd-lite core -name '*repair*' -o -name '*backfill*' -o -name '*legacy*'` prints nothing.

## Validation
- `just lint` and `just test` in atm-bd-orchestration.
- `uv run pyright --project skills/atm-bd-lite` and `uv run pyright --project core` (strict) report 0 errors.
- The bo-12 gate exits 0 on the three hooks.
