# Sprint bo-14: lite cron through `atm-bd cron`, lite install, one live lite phase

Repo: atm-bd-orchestration. Branch `sprint/bo-14-lite-live`, cut from the top of the phase-bo stack once it holds bo-9..bo-12 and bo-15. One dev; the live run (deliverable 6) is run by one agent (ANC-015).

Paths below are relative to `plugins/atm-bd-orchestration/` unless they start with `apps/` or `.atm-bd/`. The lite skill is `skills/atm-bd-lite/` (ANC-028).

## Dependencies (the artifact each one supplies)
- **bo-9**: `atm-bd-types` 0.2.0 and the bd-cache-rs 0.1.0 wheel pin (the two release-asset URLs). The hooks the live run calls use them; cron does not import atm-bd-types.
- **bo-15**: lite cron is `atm-bd cron --phase <x>` on a lite phase file, routed by `<console>/profile.py: route` to `<console>/policy_route.py`, which runs `atm_bd_core.tick.tick(policy, phase_file, ports, now, execute_actions=True, dry_run=...)` with the policy `policy.load` reads from `.claude/skills/atm-bd-lite/policy.toml`. bo-14 uses: the rule ids of the lite rule set (`qa.assign_ready`, checks 1-9 as `queue.*`/`stack.*`), the `Assign`/`Notify` actions and `execute`'s outcomes `sent`/`failed <kind>`/`dry_run`, the cron output line shape `{at, kind, to, bead, text, outcome}`, `skills/atm-bd-lite/policy.toml` and `templates/cron-report.md.j2`, the typed errors `CACHE_UNAVAILABLE` and `PROFILE_MISMATCH`, and `FakeRunner`, `FakeCache`, `make_phase_file` in `core/tests/conftest.py`.
- **bo-10**: the three installed hooks `atm-bd-task-assign-hook.py`, `atm-bd-task-start-hook.py` and `atm-bd-task-hook.py`. The live run calls them.
- **bo-11**: the dev template's stack block and the QA template's pour block. `qa.assign_ready` uses the dispatch variable contract `{task_id, phase, profile}` (bo-11 deliverable 3) and the phase-file keys `profile` and `[templates].qa` (bo-11 deliverable 5).
- **bo-12**: `skills/atm-bd-lite/scripts/atm-bd-lite-gate.py` must exit 0 on the three bo-10 hooks at the bo-14 top.
- **bo-13** (atm-core): `log_run` records `{code, message, fix, context}` and the actions of every hook reply. The live run reads those records with `atm log filter`. The atm-core release that carries bo-13 must be installed on the run host.
- **randlee/beads-cache#104**: a beads-cache release with the pour/create op, pinned by bo-9. The pour block in the run's findings round needs it.
- **A released `bdc` binary** (bo-11 Q8, blocked on randlee/sc-publish#135). The stack and pour blocks run `bdc` live, so deliverable 6 cannot start until one is installed on the run host. Deliverables 1-5 and 7 do not need it: cron reads beads through the released `bd_cache_rs` wheel in-process (bo-15).

## Lite cron
There is no lite cron script. Lite cron is `atm-bd cron --phase <x>`, the same command and arguments as orchestration's (ANC-005), running bo-15's collectors, the lite policy's rules and `execute`. bo-14 adds no routing, no rule and no `atm_bd_core` module. What bo-14 adds is the installed lite skill (its `policy.toml` included), the crontab line, the tests that drive `atm-bd cron` on a lite phase through the installed policy file, and the live run.

Beads are read through beads-cache only, through `bd_cache_rs` in-process (bo-15 collector); no `bd` and no `bdc` process (ANC-063). Cron's subprocesses are `atm`, `git` and the provider CLI, all in bo-15's collectors and executor.

## Deliverables

1. **`atm-bd cron` on a lite phase assigns ready QA** (ANC-074, ANC-005, ANC-027, ANC-031)
   - `skills/atm-bd-lite/tests/test_cron.py` drives `atm_bd_console.cli.main(["cron", "--phase", "x"])` on a temp repo holding a lite phase file and the lite skill as `atm-bd skill --lite` installs it (deliverable 3), with bo-15's `FakeRunner` and `FakeCache` and a fixed `now`.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_cron.py` passes. Cases:
     - a READY listing with one unassigned and one assigned `stage:qa` bead gives exactly one `atm task assign <qa_member> --as <lead> --task-id <bead> --template <templates.qa> --vars {"task_id":…,"phase":"x","profile":"lite"}` argv, for the unassigned bead, and one printed `assign` line with outcome `sent`;
     - `--dry-run` gives no assign or send argv and the same lines with outcome `dry_run`;
     - only already-reported keys (the fake `atm search` returns them) gives no `atm send`; one new key gives exactly one `atm send <lead> --as <lead> --template .claude/skills/atm-bd-lite/templates/cron-report.md.j2 --vars -`;
     - a phase file with no `qa_member` gives one report line with the fix `add qa_member = "<agent>" to .atm-bd/phase-x.toml` and no assign argv;
     - a phase file with `profile = "orchestration"` never reaches the policy (orchestration's `runner.step` fake is called instead); the installed `policy.toml` edited to `profile = "other"` gives `PROFILE_MISMATCH`, exit 3, no argv (ANC-027);
     - the test runs from an unrelated working directory.
   - Acceptance: `test ! -e skills/atm-bd-lite/scripts/cron && echo none` prints `none`; `grep -rn "atm-bd cron --phase" skills/atm-bd-lite/SKILL.md` prints the crontab line.

2. **Cron takes no other action** (ANC-074, ANC-024)
   - Acceptance: `grep -c '^id = "qa.assign_ready"' skills/atm-bd-lite/policy.toml` prints `1`.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_cron.py -k only_qa_assigns` passes: every rule in the installed `policy.toml`, run on bo-15's lite fixture facts, gives `Assign` actions only from `qa.assign_ready`.

3. **Lite installer** (ANC-001, ANC-028, ANC-031, ANC-012)
   - `atm-bd skill --lite [--github | --azure] [--dest REPO] [--codex]` runs `skills/atm-bd-lite/scripts/install.py`. It installs `skills/atm-bd-lite/` (with `policy.toml` and `templates/cron-report.md.j2`) to `.claude/skills/atm-bd-lite/` (or `.codex/`). `atm_bd_core` is not installed separately: it ships inside the `atm-bd` wheel (bo-15 deliverable 8).
   - The installer runs the step that puts bd_cache_rs into the `atm-bd` tool environment: `uv tool install --force atm-bd --with <wheel URL for this platform>`, and prints the command it ran. On a platform with no wheel it stops with `CACHE_UNAVAILABLE` naming the platform. Acceptance: the installer test, with `uv` faked, asserts that exact argv for macOS arm64 and linux x86_64 and the typed refusal for another platform.
   - The installer renders the chosen provider's command set with sc-compose into `skills/atm-bd-lite/templates/provider-commands.toml.j2`, from which bo-15's provider collector takes the provider. It also renders every provider placeholder in bo-11's templates; today those templates hard-code `gh pr view`/`gh pr create`/`gh pr edit` and have no placeholder (Questions).
   - The lite install writes nothing under `skills/atm-bd-orchestration/`, and the orchestration install writes nothing under `skills/atm-bd-lite/`.
   - `skills/atm-bd-lite/SKILL.md` (`name: atm-bd-lite`) gets an "Install" section and a "Cron" section. bo-11 owns the "Dispatch" section. Together they cover:
     - install;
     - set `profile = "lite"`, `lead`, `qa_member` and `[templates]` in the phase file (bo-11's `phase-file.example.toml` plus `qa_member`);
     - add the crontab line `atm-bd cron --phase <x>`;
     - assign → start → stack block → close.
   - Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_install.py` passes:
     - the installed `.claude/skills/atm-bd-lite/policy.toml` loads with bo-15's `policy.load` and no error;
     - `--azure` renders `provider-commands.toml` with `az repos pr` and no `gh pr`, and `--github` renders the reverse. The dev template is in the anchor's check too, so this part of ANC-001 waits on the provider-placeholder question;
     - `SKILL.md` names `atm-bd-lite`;
     - after a lite install, `find .claude/skills/atm-bd-orchestration` is empty;
     - an `--orchestration` install into a temp repo gives the same tree, byte for byte, as at the phase base.

4. **Orchestration not broken, not changed** (ANC-030, ANC-039, ANC-036)
   - Acceptance: `git diff --name-only <phase base>..HEAD -- plugins/atm-bd-orchestration/skills/atm-bd-orchestration/` prints nothing.
   - Acceptance: `git diff --diff-filter=MD --name-only <phase base>..HEAD -- plugins/atm-bd-orchestration/tests apps/atm-bd-console/tests '*/tests/test_*'` prints nothing. Tests are only added, never modified or deleted.
   - Acceptance: `just test` passes at the bo-14 top. That run includes orchestration's whole suite: console pytest, `.github/scripts/tests`, the `gen_manifest.py --check` and the plugin pytest. `<phase base>` is the commit the phase-bo stack was cut from, recorded in the PR body.
   - Acceptance: `git log --follow --name-status --format= -- core/atm_bd_core/` shows only `A` entries, no `R`/`C` from a `skills/atm-bd-orchestration/` path.

5. **No shared code added here** (ANC-036, ANC-040, ANC-041, ANC-042, ANC-044, ANC-045)
   - bo-14 adds no module to `core/atm_bd_core/`; the shared layer is bo-15's.
   - Acceptance: `git diff --name-only --diff-filter=A <bo-15 top>..HEAD -- core/atm_bd_core` prints nothing, and `uv run --project core pytest -q core/tests` (bo-15's inventory included) passes at the bo-14 top.

6. **One live lite phase, end to end** (ANC-015, ANC-074, ANC-018)
   - **Release.** Release atm-bd as a Python-only version carrying bo-9..bo-14, and install it from the release assets (`atm-bd skill --lite --github`). `gh run view <release run> --json jobs --jq '.jobs[].name'` lists no crate publish job (ANC-018).
   - **Setup.** In a phase file with `profile = "lite"`, one ATM identity is `lead` and `qa_member`. A crontab runs `atm-bd cron --phase <x>` (the same line as an orchestration phase). The phase has one sprint whose dev bead is a real open atm-bd-orchestration issue.
   - **Sequence:**
     1. Lead `atm task assign` (dev).
     2. `atm task start`.
     3. Dev work.
     4. The stack block.
     5. `atm task close completed`, which goes through the hook as T-CLOSE-02.
     6. A cron tick assigns the released QA bead (deliverable 1).
     7. QA with findings: the pour block, then close as T-CLOSE-04.
     8. Lead assigns the fix bead (`queue.ready_work_no_task`, check 4, reports it ready with no task).
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

7. **No repair scripts** (ANC-059)
    - Acceptance: `find skills/atm-bd-lite core -name '*repair*' -o -name '*backfill*' -o -name '*legacy*'` prints nothing.

## Validation
- `just lint` and `just test` in atm-bd-orchestration.
- `uv run pyright --project skills/atm-bd-lite` (strict) reports 0 errors.
- The bo-12 gate exits 0 on the three hooks.
