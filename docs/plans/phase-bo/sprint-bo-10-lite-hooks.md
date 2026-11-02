# Sprint bo-10: the three atm-bd-lite hooks

Repo: atm-bd-orchestration. Branch `sprint/bo-10-lite-hooks`, cut from the stack top once bo-9 and bo-12 are on it. One dev.

The spec is `hooks/task-assign.md`, `hooks/task-start.md` and `hooks/task-close.md` (Rand-owned, ANC-071). Each hook implements its spec and nothing more. A guard, write, arm or read that its spec does not list does not go in, whoever asks (ANC-072). If the spec is wrong, the dev stops and asks Rand. The dev does not fix it in code.

## Consumes

- **bo-12**: the gate `skills/atm-bd-lite/scripts/atm-bd-lite-gate.py <hook.py> <hooks/spec.md> [--out run.json]` (design copy `design/lite-code/atm-bd-lite-gate.py`) and its package `packages/jev-check`. Exit 0 = pass, 1 = a finding or a Jev `no`, 2 = Jev could not be asked (not a pass). The gate parses `Transition.X` match arms, `beads.update/create/claim/release(` call sites, `stack_bead` occurrences and `HookReply.*` sites. So each hook names its store parameter `beads` and its enum `Transition`.
- **bo-9**: `atm_bd_types` 0.2.0 (the generated stub). The hooks use these names:
  - `HookRequest`: `parse`, `is_phase_task`, `phase_config_path`, `caller`, `task_id`, `assignee`, `outcome`, `close_report`, `close_reason`, `qa_verdict`.
  - `HookReply`: `not_applicable`, `pass_through`, `refused`, `handled`, `to_json`.
  - `Refusal`, carrying a fix text per (ErrorId, hook) that equals the hook's Errors table.
  - `ErrorId`: every code in the three Errors tables, plus `STACK_NOT_RECORDED` and `FINDINGS_NOT_POURED`.
  - `Bead`, `Status`, `Stage`, `Outcome`, `Dependency`, `Work`, `BeadMeta`, `Stack`, `Layer`, `Sha`, `QaPass`/`QaFail`.
  - `BeadStore` with `show` and `update` only, and `CacheStore.for_phase`.
  - `BeadPatch.assign`, `BeadPatch.start(snapshot)` (no base: start does not read the ledger), `BeadPatch.reopen`, `BeadPatch.close` with evidence.
  - `PhaseFiles`, `PhaseFilesCli`, `PhaseFile` with `lead` and the four close templates (`<refuse>`, `<dev-close>`, `<qa-pass>`, `<qa-findings>`).
  - The `HookAction` that renders `atm task close --template <t> --args <a>`.

  - One transition enum per hook, with exactly that hook's Transitions-table names. For example, `AssignTransition` has ASSIGN_SAME, ASSIGN, ASSIGN_CLOSED and ASSIGN_NOT_READY. Each hook imports its enum `as Transition`. The gate's patterns therefore read `Transition.X`, and pyright's exhaustiveness check covers only that hook's members.

  If the stub lacks a name on this list, bo-9 is not done. bo-10 does not add the name itself.
- **bo-11**: the data its fenced blocks write. The close hook checks this data and never produces it. Three items:
  - the stack block's ledger layer on the stack bead (branch, head, pr, base), read through `Stack`/`Layer`;
  - the pour block's dependency edges (each finding bead blocks the sprint bead or the phase bead);
  - the QA report's fenced verdict block `{"verdict", "sha", "findings": [ids]}`.

  The tests build these with fakes, in the shapes bo-11 defines.

No shared-location module is added. The hooks import only `sys`, `atm_bd_types` and, through `CacheStore`, `bd_cache_rs` (ANC-040/045 add nothing here).

## Hook shape (all three)
This is the shape the gate checks.
1. **Preconditions.** The guards from the spec's diagram run first. Each returns early with `HookReply.refused(...)` or `not_applicable()`. These guards are not match arms.
2. **Selection.** The transition is computed into a value, `transition = ...`, from the status, outcome and stage. This step is not a match arm and has no refusal site of its own.
3. **Match.** `match transition:` has one `case Transition.X:` per Transitions row and no wildcard. Each arm runs only its row's "hook checks". It either returns a refusal or yields `(patch, reply)`. `patch` is `None` for a row whose write is "none".
4. **Write.** After the match, there is exactly one `beads.update(patch, ...)` call site, reached only when `patch` is not `None`. A failed guarded write returns its refusal (`BEAD_RACED`). Otherwise the hook returns the arm's `reply`.

So each file has at most one write call site (ANC-064), while each of the four close arms still makes its own write.

## Deliverables

1. **`skills/atm-bd-lite/scripts/atm-bd-task-assign-hook.py`**. Implements `hooks/task-assign.md` (T-ASSIGN-01..04, its Errors table). Anchors: ANC-048, 003, 016, 026, 056, 058, 062, 063, 064, 065, 066, 070.
   Acceptance: `uv run skills/atm-bd-lite/scripts/atm-bd-lite-gate.py skills/atm-bd-lite/scripts/atm-bd-task-assign-hook.py <plan>/hooks/task-assign.md` exits 0. `<plan>` is atm-core `docs/plans/phase-bo` at the plan commit. The run JSON goes to `anchor-runs/hooks/atm-bd-task-assign-hook.json`.
2. **`skills/atm-bd-lite/scripts/atm-bd-task-start-hook.py`**. Implements `hooks/task-start.md` (T-START-01..04, its Errors table). The hook reads only the phase file and the bead. It never reads the stack bead. Anchors: ANC-049, 003, 016, 026, 056, 058, 062, 063, 064, 065, 066, 070.
   Acceptance: the gate on this file and `<plan>/hooks/task-start.md` exits 0. `grep -n stack skills/atm-bd-lite/scripts/atm-bd-task-start-hook.py` prints nothing.
3. **`skills/atm-bd-lite/scripts/atm-bd-task-hook.py`** (task.close). Implements `hooks/task-close.md` (T-CLOSE-01..04, its Errors table). The hook makes one write, to the caller's own bead. It reads the stack bead to check the dev's layer, and the poured finding beads to check what they block. It never writes the stack bead, creates a bead or assigns a task. Anchors: ANC-050, 003, 014, 016, 019, 026, 056, 058, 062, 063, 064, 065, 066, 070.
   Acceptance: the gate on this file and `<plan>/hooks/task-close.md` exits 0. `wc -l skills/atm-bd-lite/scripts/atm-bd-task-hook.py` reports ≤ 100 (ANC-070). No line is dropped from the spec to fit; if the spec cannot fit, the dev stops and asks Rand. `grep -c 'beads.update(' skills/atm-bd-lite/scripts/atm-bd-task-hook.py` prints 1. `git diff --exit-code <bo-10 base> -- skills/atm-bd-orchestration/` prints nothing, so orchestration's `atm-task-close.py` is unchanged and has no lite branch (ANC-014).
4. **`skills/atm-bd-lite/tests/fakes.py`**. In-memory `FakeBeads` (implements `BeadStore.show`/`update`) and `FakePhases` (implements `PhaseFiles.load`). Each fake records every lookup and every write in order. A fake can be told to return a given `Refusal` for a given lookup, or `BEAD_RACED` for a write. No sleeps, threads, clocks, bd, dolt, gh, git or daemon.
5. **Transition tests**: one test per Transitions row, listed below. Each test calls `hook()` with fakes and asserts three things exactly: the reply JSON, the list of writes, and the list of lookups. Anchors: ANC-052/053 (behavioural side).
   Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_assign_hook.py skills/atm-bd-lite/tests/test_start_hook.py skills/atm-bd-lite/tests/test_close_hook.py -v` passes and lists every test named below.
6. **`skills/atm-bd-lite/tests/test_errors.py`**: one test per Errors-table row per hook, listed below. A grouped row is parametrized over its codes. Each test asserts the refused reply's code, its owner and its fix text. The expected fix text is the spec's fix column with the placeholders filled from the fakes, compared character for character. Anchors: ANC-058.
   Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_errors.py -v` passes. No test supplies stored state.
7. **`skills/atm-bd-lite/tests/test_hook_facts.py`**. For each hook, the request carries the task id and the ATM request fields only. The test asserts that the lookup log contains only the rows of that hook's Facts table: the phase file, `cache.get(bead)`, plus `cache.get(stack_bead)` and `cache.get(id)` per finding for close. Anchors: ANC-055, 056.
   Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_hook_facts.py` passes. `grep -rnE 'close-vars|close_vars|open\(|read_text|write_text' skills/atm-bd-lite/scripts/atm-bd-task-*.py` prints nothing.
8. **`skills/atm-bd-lite/tests/test_lite_profile_gate.py`** (hook part of ANC-026). Each hook gets a real `PhaseFilesCli` on a tmp phase file whose profile is not lite. It returns `refused PROFILE_MISMATCH`, with no bead lookup and no write. The profile is not taken from argv or env. Anchors: ANC-026.
   Acceptance: `uv run pytest skills/atm-bd-lite/tests/test_lite_profile_gate.py` passes. `grep -nE 'argv|environ' skills/atm-bd-lite/scripts/atm-bd-task-*.py` shows only `HookRequest.parse(sys.stdin...)` in `__main__`.
9. **`skills/atm-bd-lite/tests/test_transition_exhaustive.py`** and **`test_one_handler_per_transition.py`**. Two tests:
   - Pyright passes on each hook. The first test copies a hook, deletes one `case Transition.X`, and asserts that pyright reports `reportMatchNotExhaustive` (ANC-003).
   - Using `ast`, each hook imports its bo-9 enum `as Transition` and defines no enum of its own. The enum's members equal the spec's transition column. The hook has exactly one `match transition:`, with one `case Transition.X` per member and no wildcard. It has exactly one `beads.update(` call site, after the match (ANC-019, the hook shape above).

   Anchors: ANC-003, 019.
   Acceptance: `npx pyright -p skills/atm-bd-lite` reports 0 errors with `strict` and `reportMatchNotExhaustive = "error"` in `skills/atm-bd-lite/pyrightconfig.json`. `uv run pytest skills/atm-bd-lite/tests/test_transition_exhaustive.py skills/atm-bd-lite/tests/test_one_handler_per_transition.py` passes.
10. **Static greps** on the three hook files. Each command prints nothing:
    - `grep -nE '\braise\b' skills/atm-bd-lite/scripts/atm-bd-task-*.py` (ANC-016: every function returns a value or a `Refusal`).
    - `grep -nE 'dict\[|\bAny\b|json\.loads' skills/atm-bd-lite/scripts/atm-bd-task-*.py` (ANC-062).
    - `grep -nE 'subprocess|BdCli|GitCli|AtmCli|"bd |"gh |"git ' skills/atm-bd-lite/scripts/atm-bd-task-*.py` (ANC-063, 066).
    - `grep -rnE 'gh pr list|gh pr checks|gh run watch|--watch' skills/atm-bd-lite` (ANC-060, hook part).
    - `grep -nE 'NewBead|\.create\(|\.claim\(|\.release\(|BeadPatch\.record|STACK_HELD|STACK_CLAIM_LOST|qa_handoff|HookAction\.assign' skills/atm-bd-lite/scripts/atm-bd-task-*.py` (ANC-064, 065: retired designs).
11. **One file per hook, one diagram per hook**:
    - `ls skills/atm-bd-lite/scripts/atm-bd-task-*.py` lists exactly the three files above.
    - `grep -nE '^(from|import) .*atm.bd.task' skills/atm-bd-lite/scripts/atm-bd-task-*.py` prints nothing: no hook imports another hook (ANC-048, 049, 050).
    - Each hook's docstring names its `hooks/task-*.md`. `grep -c '^```mermaid' <plan>/hooks/task-assign.md <plan>/hooks/task-start.md <plan>/hooks/task-close.md` prints 1 for each file (ANC-051: the hooks/*.md files are the diagrams; bo-10 does not edit them).
    - `wc -l skills/atm-bd-lite/scripts/atm-bd-task-*.py` reports at most 100 lines per file (ANC-070, also enforced by the gate).

## Transitions rows → tests (deliverable 5)

**task.assign** (`test_assign_hook.py`)
| row | test | asserts |
|---|---|---|
| T-ASSIGN-01 ASSIGN_SAME | `test_t_assign_01_same_agent_pass_through[open,in_progress]` | pass-through, no write |
| T-ASSIGN-02 ASSIGN | `test_t_assign_02_assign_guarded[other,none]` | one `BeadPatch.assign`, pass-through |
| T-ASSIGN-02 ASSIGN (race) | `test_t_assign_02_raced` | the guarded write fails → refused `BEAD_RACED` |
| T-ASSIGN-03 ASSIGN_CLOSED | `test_t_assign_03_closed` | refused `BEAD_CLOSED`, no write |
| T-ASSIGN-04 ASSIGN_NOT_READY | `test_t_assign_04_not_ready[blocked,deferred]` | refused `BEAD_NOT_READY` naming the status, no write |
| diagram: not a phase task | `test_assign_not_phase_task` | not-applicable, no lookup |
| diagram: open blockers | `test_assign_open_blockers` | refused `BEAD_NOT_READY` naming the blocker ids, no write |

**task.start** (`test_start_hook.py`)
| row | test | asserts |
|---|---|---|
| T-START-01 START | `test_t_start_01_start_guarded` | one `BeadPatch.start(snapshot)`, pass-through. Lookups: phase file, bead (no stack bead) |
| T-START-01 START (race) | `test_t_start_01_raced` | refused `BEAD_RACED` |
| T-START-02 START_AGAIN | `test_t_start_02_in_progress_pass_through` | pass-through, no write |
| T-START-03 START_CLOSED | `test_t_start_03_closed` | refused `BEAD_CLOSED`, no write |
| T-START-04 START_NOT_READY | `test_t_start_04_not_ready[blocked,deferred]` | refused `BEAD_NOT_READY`, no write |
| diagram: not a phase task | `test_start_not_phase_task` | not-applicable, no lookup |
| diagram: caller not assignee | `test_start_not_assignee` | refused `TASK_NOT_ASSIGNEE`, no write |
| diagram: open blockers | `test_start_open_blockers` | refused `BEAD_NOT_READY`, no write |

**task.close** (`test_close_hook.py`)
| row | test | asserts |
|---|---|---|
| T-CLOSE-01 GIVE_BACK | `test_t_close_01_give_back[refused,cancelled × dev,fix,qa]` | one `BeadPatch.reopen` (open, assignee ''), handled `atm task close --template <refuse> --args <reason>` |
| T-CLOSE-01 reason empty | `test_t_close_01_reason_empty` | refused `CLOSE_REASON_EMPTY`, no write |
| T-CLOSE-01 caller check | `test_t_close_01_not_assignee` | refused `TASK_NOT_ASSIGNEE`, no write |
| T-CLOSE-01 status check | `test_t_close_01_not_in_progress[open,blocked,deferred]` → `BEAD_NOT_READY`; `test_t_close_01_closed` → `BEAD_CLOSED` | no write |
| T-CLOSE-01 race | `test_t_close_01_raced` | refused `BEAD_RACED` |
| T-CLOSE-02 WORK_CLOSE | `test_t_close_02_work_close[dev,fix]` | lookups: bead, `cache.get(stack_bead)`. One `BeadPatch.close` on the work bead with evidence = the ledger layer (branch, head, pr, base). Handled `--template <dev-close> --args <layer>`. No write names the stack bead |
| T-CLOSE-02 report missing | `test_t_close_02_report_missing` | refused `CLOSE_REPORT_NOT_INLINE`, no write |
| T-CLOSE-02 not on ledger | `test_t_close_02_stack_not_recorded` | the ledger has no layer for `work.branch` → refused `STACK_NOT_RECORDED`, no write |
| T-CLOSE-03 QA_PASS | `test_t_close_03_qa_pass_blocker_layer` | verdict sha == the head of the closed dev/fix blocker's layer → one `BeadPatch.close` with sha, handled `--template <qa-pass> --args <sha>` |
| T-CLOSE-03 checked_bead | `test_t_close_03_qa_pass_checked_bead` | the target is the layer of `checked_bead` when it is set |
| T-CLOSE-03 sha mismatch | `test_t_close_03_not_on_stack` | refused `QA_NOT_ON_STACK`, no write |
| T-CLOSE-03 no target | `test_t_close_03_nothing_to_review` | refused `QA_NOTHING_TO_REVIEW`, no write |
| T-CLOSE-03 verdict invalid | `test_t_close_03_verdict_invalid` | refused `QA_VERDICT`, no write |
| T-CLOSE-04 QA_FINDINGS | `test_t_close_04_findings_poured` | lookups: bead, stack bead, `cache.get(id)` per listed finding. Every finding blocks its bead → one `BeadPatch.close` with sha + ids, handled `--template <qa-findings> --args <sha>, <ids>`. No create |
| T-CLOSE-04 not poured | `test_t_close_04_finding_missing` | a listed id is not in the cache → refused `FINDINGS_NOT_POURED` naming it, no write |
| T-CLOSE-04 wrong blocker | `test_t_close_04_finding_blocks_nothing` | a finding blocks neither the sprint bead nor the phase bead → refused `FINDINGS_NOT_POURED`, no write |
| T-CLOSE-04 sha mismatch | `test_t_close_04_not_on_stack` | refused `QA_NOT_ON_STACK`, no write |
| diagram: not a phase task | `test_close_not_phase_task` | not-applicable, no lookup |

## Errors rows → tests (deliverable 6, `test_errors.py`)

The expected text is the spec's fix column with the placeholders filled in. In the examples below, `bo-1` is the bead, `bo-2, bo-3` are the blockers, `dev-a` is the assignee and `lead-a` is the lead.

**task.assign**
| code | test | expected fix |
|---|---|---|
| ASSIGN_NOT_LEAD | `test_assign_err_not_lead` | `Only the lead (lead-a) assigns. Ask the lead.` |
| BEAD_NOT_READY | `test_assign_err_not_ready[blockers,status]` | `bo-1 is blocked by bo-2, bo-3. Close the blockers first.` / `bo-1 is deferred. Close the blockers first.` |
| BEAD_CLOSED | `test_assign_err_closed` | `bo-1 is closed. Nothing to assign.` |
| REQUEST_INVALID, BEAD_NOT_FOUND, BEAD_RACED, PHASE_UNREADABLE, PROFILE_MISMATCH, CACHE_UNAVAILABLE | `test_assign_err_lead_row[<code>]` | `<what failed>. Retry or fix the phase.`, owner lead |

**task.start**
| code | test | expected fix |
|---|---|---|
| TASK_NOT_ASSIGNEE | `test_start_err_not_assignee` | `bo-1 is assigned to dev-a, not you. Start only your own task.` |
| BEAD_NOT_READY | `test_start_err_not_ready[blockers,status]` | `bo-1 is blocked by bo-2, bo-3. Wait for the lead.` / `bo-1 is deferred. Wait for the lead.` |
| BEAD_CLOSED | `test_start_err_closed` | `bo-1 is closed. Nothing to start.` |
| BEAD_NOT_FOUND, BEAD_RACED, PHASE_UNREADABLE, PROFILE_MISMATCH, CACHE_UNAVAILABLE | `test_start_err_lead_row[<code>]` | `Not yours to fix: <what failed>. The lead has it.`, owner lead |

**task.close**
| code | test | expected fix |
|---|---|---|
| STACK_NOT_RECORDED | `test_close_err_stack_not_recorded` | `sprint/x-1 is not on the ledger. Run the stack block in your task, then close again.` |
| CLOSE_REPORT_NOT_INLINE / CLOSE_REASON_EMPTY | `test_close_err_report_or_reason[report,reason]` | ``Run `atm task close bo-1 completed --stdin` with the report (or reason).`` (outcome filled from the request) |
| TASK_NOT_ASSIGNEE | `test_close_err_not_assignee` | `bo-1 is assigned to dev-a, not you.` |
| BEAD_CLOSED | `test_close_err_closed` | `bo-1 is already closed.` |
| BEAD_NOT_READY | `test_close_err_not_ready` | ``bo-1 is open; `atm task start` it first.`` |
| QA_VERDICT | `test_close_err_qa_verdict` | the spec text verbatim |
| QA_NOT_ON_STACK | `test_close_err_not_on_stack` | `Your verdict is for <sha>; sprint/x-1 is at <head>. Review <head>, then close again.` (full 40-hex shas) |
| FINDINGS_NOT_POURED | `test_close_err_findings_not_poured` | `Findings bo-7, bo-8 are not poured or do not block bo-s. Run the pour block, then close again.` |
| QA_NOTHING_TO_REVIEW, BEAD_NOT_FOUND, BEAD_RACED, PHASE_UNREADABLE, PROFILE_MISMATCH, CACHE_UNAVAILABLE | `test_close_err_lead_row[<code>]` | `Not yours to fix: <what failed>. The lead has it.`, owner lead |

`<what failed>` is the refusal's message from bo-9. The test asserts the whole string: the message, then the row's literal suffix. Rows whose fix text names no command ("The lead has it", "Wait for the lead", "Retry or fix the phase") are asserted exactly as the spec writes them. The tests do not add a command the spec does not give.

## Anchors → deliverables
| anchor | deliverable(s) |
|---|---|
| ANC-003 | 1–3, 9 |
| ANC-014 | 3 |
| ANC-016 | 10 |
| ANC-019 | 3, 9 |
| ANC-026 (hook part) | 8 |
| ANC-048 / 049 / 050 | 1 / 2 / 3, 11 |
| ANC-051 | 11 |
| ANC-052 / 053 / 054 | 1–3 (the bo-12 gate exits 0 on each hook), 5 |
| ANC-055 | 7 |
| ANC-056 | 7 |
| ANC-058 | 6 |
| ANC-060 (hook part) | 10 |
| ANC-062 | 9 (pyright strict), 10 |
| ANC-063 | 10 |
| ANC-064 / 065 / 066 | 1–3 (gate), 10 |
| ANC-070 | 1–3 (gate), 11 |
| ANC-072 | 1–3. The gate runs in CI on every push to this branch. A review-driven change that adds an arm, write, guard or subprocess fails it |

## Not in bo-10
The cron (bo-14). The stack block and the pour block (bo-11 templates). The gate and jev-check (bo-12). Hook registration and install (bo-14). The typed refusal on the wire (bo-13). `design/lite-code/docs/state-transitions.md` is not installed or copied: the three `hooks/*.md` files are the diagrams (ANC-051).

## Validation
- `uv run pytest skills/atm-bd-lite/tests`
- `npx pyright -p skills/atm-bd-lite`
- The gate on all three hooks (deliverables 1–3)
- `just lint`
