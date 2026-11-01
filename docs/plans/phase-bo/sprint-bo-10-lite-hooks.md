# Sprint bo-10: atm-bd-lite hooks (one sprint)

Repo: atm-bd-orchestration. Branch `sprint/bo-10-lite-hooks`, stacked on bo-9. One dev.

## Deliverables (implement as written; any change is a plan change and goes to fenix first)

| file (installed path) | source in this plan | lines |
|---|---|---|
| `.atm-bd/atm-bd-lite/scripts/atm-bd-task-start-hook.py` | `design/lite-code/atm-bd-task-start-hook.py` | 52 |
| `.atm-bd/atm-bd-lite/scripts/atm-bd-task-assign-hook.py` | `design/lite-code/atm-bd-task-assign-hook.py` | 45 |
| `.atm-bd/atm-bd-lite/scripts/atm-bd-task-hook.py` (task.close) | `design/lite-code/atm-bd-task-hook.py` | 115 |
| `.atm-bd/atm-bd-lite/scripts/atm-bd-lite-cron.py` | `design/lite-code/atm-bd-lite-cron.py` | 31 |
| `.atm-bd/shared/` (package `atm-bd-shared`, `qa_handoff`) | `design/lite-code/shared/` | 46 |
| `skills/atm-bd-lite/docs/state-transitions.md` | `design/lite-code/docs/state-transitions.md` | — |

## Rules
- Beads go through beads-cache only (`CacheStore`, ANC-063). No `bd`, `gh` or `git` subprocess calls.
- Pyright strict, 0 errors (`design/lite-code/pyrightconfig.json`). Typed (ANC-062).
- Tests drive `hook()` with in-memory fakes of `BeadStore` and `PhaseFiles`. No sleeps, threads, timing, real bd, dolt or daemon.
- Each test asserts the exact reply JSON and the exact list of writes.
- Each lite script's uv header names `atm-bd-shared = { path = "../../shared" }`. One test runs the installed close hook from an unrelated working directory.

## Acceptance: one test per transition row of `state-transitions.md`

**task.start**
- [ ] OPEN → one `BeadPatch.start(snapshot, base)`. Base = the trunk on a new stack, else the top layer's branch. The stack bead is only read. A precondition failure gives `BEAD_RACED`.
- [ ] Caller not the assignee: `TASK_NOT_ASSIGNEE`. Open blocker: `BEAD_NOT_READY`. IN_PROGRESS: pass-through. CLOSED: `BEAD_CLOSED`. BLOCKED or DEFERRED: `BEAD_NOT_READY`.

**task.assign**
- [ ] Caller not the lead: `ASSIGN_NOT_LEAD`. Open blocker: `BEAD_NOT_READY`. Already this agent: pass-through. Otherwise one `BeadPatch.assign`. CLOSED: `BEAD_CLOSED`.
- [ ] The QA assigns returned by the close hook (actor = lead) are admitted.

**task.close** (T-CLOSE-01..07)
- [ ] 01: refused or cancelled → one `BeadPatch.reopen` (open, unassigned).
- [ ] 02: no valid fenced layer block → `LAYER_BLOCK_MISSING`, no write.
- [ ] 03: stack held → `STACK_HELD`. Block base ≠ top → claim, release, then `STACK_NOT_STACKED` naming the top.
- [ ] 04: base = top (the trunk on a new stack) → claim, one `BeadPatch.record` (layer + release), one `BeadPatch.close`, one assign per ready QA bead, then the original close.
- [ ] 05: verdict sha ≠ target head → `QA_NOT_ON_STACK`.
- [ ] 06: PASS → one `BeadPatch.close(reviewed=sha)`.
- [ ] 07: FAIL → one cache `create` of the fix bead (blocks QA, owner = dev), then one assign to the dev. Needs randlee/beads-cache#104 released and pinned in bo-9.

**cron**
- [ ] It assigns each ready, unassigned QA bead once. An already-assigned QA bead is skipped.

**errors**
- [ ] Each row of the error table: its code, its owner, and a message containing the exact fix command.

## Validation
`just lint`, `just test`, `npx pyright -p design/lite-code`.
