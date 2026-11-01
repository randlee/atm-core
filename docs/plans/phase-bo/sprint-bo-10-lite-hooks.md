# Sprint bo-10: atm-bd-lite hooks (one sprint)

Repo: atm-bd-orchestration. Branch `sprint/bo-10-lite-hooks`, stacked on bo-9. One dev.

> **NOT APPROVED.** The code in this sprint is fenix's draft. Rand has not approved it. It must not be dispatched or implemented until Rand approves it.

## Deliverables (draft)

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

## Code: task.start hook
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.start hook: the assignee starts an unblocked bead; its base is the stack top at start (the trunk
for a new stack), recorded on the bead. No claim: the dev claims the stack only when its work is complete."""
import sys

from atm_bd_types import CacheStore, BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli, Refusal, Stack, Status

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if not bead.assigned_to(req.caller):
        return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead, caller=req.caller))
    if bead.open_blockers():
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
    match bead.status:
        case Status.OPEN:  # the work's base is the stack top now; no claim (the dev claims only at completion)
            if isinstance(work := bead.work(), Refusal):
                return HookReply.refused(work)
            if isinstance(stack_id := bead.stack_bead(), Refusal):
                return HookReply.refused(stack_id)
            if isinstance(stack_bead := beads.show(stack_id), Refusal):
                return HookReply.refused(stack_bead)
            if isinstance(stack := Stack.decode(stack_bead), Refusal):
                return HookReply.refused(stack)
            return HookReply.from_result(beads.update(BeadPatch.start(bead, base=stack.top(excluding=work.branch)), actor=req.caller))
        case Status.IN_PROGRESS:
            return HookReply.pass_through()
        case Status.CLOSED:
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(beads := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(beads).to_json())
    else:
        print(hook(req, beads, PhaseFilesCli()).to_json())
```

## Code: task.assign hook
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.assign hook: the lead assigns a ready bead, and the cache records the assignee."""
import sys

from atm_bd_types import CacheStore, BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli, Refusal, Status

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(agent := req.assignee, Refusal):
        return HookReply.refused(agent)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if req.caller != phase.lead:
        return HookReply.refused(Refusal(ErrorId.ASSIGN_NOT_LEAD, bead=bead, phase=phase))
    if bead.open_blockers():
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:
            if bead.assigned_to(agent):
                return HookReply.pass_through()
            return HookReply.from_result(beads.update(BeadPatch.assign(bead, agent), actor=req.caller))
        case Status.CLOSED:
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(beads := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(beads).to_json())
    else:
        print(hook(req, beads, PhaseFilesCli()).to_json())
```

## Code: task.close hook
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0", "atm-bd-shared"]
# [tool.uv.sources]
# atm-bd-shared = { path = "../../shared" }  # .atm-bd/atm-bd-lite/scripts -> .atm-bd/shared
# ///
"""atm-bd-lite task.close hook. Implements docs/task-close.md, nothing else.

Work bead (dev/fix): the report's fenced layer block is stacked on the top -> record the layer, close the bead,
assign ready QA beads. Beads only through beads-cache (ANC-063).
QA bead: the verdict sha is the target layer's head -> PASS closes QA; FAIL creates a fix bead for the dev.
Refused or cancelled: the bead goes back to open, unassigned.
"""
import sys
from atm_bd_shared.qa_handoff import qa_handoff, qa_target
from atm_bd_types import (Bead, CacheStore, BeadMeta, BeadPatch, BeadStore, ErrorId, HookAction, HookReply, HookRequest,
                          NewBead, Outcome, PhaseFile, PhaseFiles, PhaseFilesCli, QaFail, QaPass, Refusal,
                          Stack, Stage, TaskAssign, Template)

PROFILE = "lite"
FIX_ID = "{qa}-fix-{sha}"
FIX_BRANCH = "fix/{fix}"


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if isinstance(outcome := req.outcome, Refusal):
        return HookReply.refused(outcome)
    match outcome:
        case Outcome.REFUSED | Outcome.CANCELLED:  # T-CLOSE-01: give the bead back
            if isinstance(reason := req.close_reason(), Refusal):
                return HookReply.refused(reason)
            return HookReply.from_result(beads.update(BeadPatch.reopen(bead, reason), actor=req.caller))
        case Outcome.COMPLETED:
            if not bead.assigned_to(req.caller):
                return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead, caller=req.caller))
            if isinstance(report := req.close_report(), Refusal):
                return HookReply.refused(report)
            if isinstance(stage := bead.stage, Refusal):
                return HookReply.refused(stage)
            match stage:
                case Stage.DEV | Stage.FIX:
                    return work_close(req, phase, bead, beads)
                case Stage.QA:
                    return qa_close(req, phase, bead, beads)


def work_close(req: HookRequest, phase: PhaseFile, bead: Bead, beads: BeadStore) -> HookReply:
    if isinstance(layer := req.layer_block(), Refusal):  # T-CLOSE-02
        return HookReply.refused(layer)
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return HookReply.refused(stack_id)
    if isinstance(stack_bead := beads.claim(stack_id, req.caller), Refusal):  # another close is stacking: close again
        return HookReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return HookReply.refused(stack)
    if layer.base != (top := stack.top(excluding=layer.branch)):  # T-CLOSE-03: rebase onto the top
        if (released := beads.release(stack_id, req.caller)) is not None:
            return HookReply.refused(released)
        return HookReply.refused(Refusal(ErrorId.STACK_NOT_STACKED, layer=layer, top=top))
    if (failed := beads.update(BeadPatch.record(stack_bead, stack.with_layer(layer), req.caller),
                               actor=req.caller)) is not None:  # layer + release, one write
        return HookReply.refused(failed)
    if (failed := beads.update(BeadPatch.close(bead, f"{layer.branch} at {layer.head}; PR #{layer.pr}"),
                               actor=req.caller)) is not None:
        return HookReply.refused(failed)
    if isinstance(assigns := qa_handoff(req.phase, phase, beads), Refusal):  # T-CLOSE-04
        return HookReply.refused(assigns)
    return HookReply.handled([HookAction.assign(req, a) for a in assigns] + [HookAction.original(req)])


def qa_close(req: HookRequest, phase: PhaseFile, bead: Bead, beads: BeadStore) -> HookReply:
    if isinstance(verdict := req.qa_verdict(), Refusal):
        return HookReply.refused(verdict)
    if isinstance(target := qa_target(bead, beads), Refusal):
        return HookReply.refused(target)
    checked, layer = target
    if layer.head != verdict.sha:  # T-CLOSE-05
        return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, checked=layer.branch))
    match verdict:
        case QaPass():  # T-CLOSE-06
            return HookReply.from_result(
                beads.update(BeadPatch.close(bead, f"PASS at {verdict.sha}", reviewed=verdict.sha), actor=req.caller))
        case QaFail(report=report):  # T-CLOSE-07 (BLOCKED: beads-cache has no create op (randlee/beads-cache#104))
            if isinstance(work := checked.work(), Refusal):
                return HookReply.refused(work)
            if isinstance(stack_id := bead.stack_bead(), Refusal):
                return HookReply.refused(stack_id)
            if isinstance(dev := checked.owner(), Refusal):
                return HookReply.refused(dev)
            fix_id = FIX_ID.format(qa=bead.id, sha=verdict.sha)
            new = NewBead(id=fix_id, title=f"fix {bead.id} at {verdict.sha.short()}", description=report,
                          parent=bead.id, phase=req.phase, stage=Stage.FIX, assignee=dev, blocks=bead.id,
                          meta=BeadMeta(work=work.sibling(FIX_BRANCH.format(fix=fix_id)), stack_bead=stack_id,
                                        qa_bead=bead.id, reviewed=verdict.sha))
            if (created := beads.create(new, actor=req.caller)) is not None and created.id != ErrorId.BEAD_EXISTS:
                return HookReply.refused(created)
            assign = TaskAssign(task=fix_id, recipient=dev, actor=phase.lead, template=phase.template(Template.FIX))
            return HookReply.handled([HookAction.assign(req, assign), HookAction.original(req)])


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(beads := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(beads).to_json())
    else:
        print(hook(req, beads, PhaseFilesCli()).to_json())
```

## Code: cron
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0", "atm-bd-shared"]
# [tool.uv.sources]
# atm-bd-shared = { path = "../../shared" }  # .atm-bd/atm-bd-lite/scripts -> .atm-bd/shared
# ///
"""atm-bd-lite cron: hand ready QA beads to the QA member (safety net for a close whose actions never ran).

  atm-bd-lite-cron.py --phase P
"""
import sys

from atm_bd_types import AtmCli, CacheStore, CronCommand, CronReply, PhaseFilesCli, Refusal
from atm_bd_shared.qa_handoff import qa_handoff

PROFILE = "lite"  # policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)

if __name__ == "__main__":
    if isinstance(cmd := CronCommand.parse(sys.argv[1:]), Refusal):
        reply = CronReply.refused(cmd)
    elif isinstance(phase := PhaseFilesCli().load(cmd.phase_config_path(), PROFILE), Refusal):
        reply = CronReply.refused(phase)
    elif isinstance(beads := CacheStore.for_phase(cmd.phase), Refusal):
        reply = CronReply.refused(beads)
    elif isinstance(assigns := qa_handoff(cmd.phase, phase, beads), Refusal):
        reply = CronReply.refused(assigns)
    else:
        tasks = AtmCli(timeout_seconds=10)
        reply = CronReply.assigned([(assign, tasks.assign(assign)) for assign in assigns])
    print(reply.to_json())
```

## Code: shared qa_handoff (`.atm-bd/shared/src/atm_bd_shared/qa_handoff.py`)
```python
"""Shared (.atm-bd/shared, package atm-bd-shared): hand ready QA beads to the phase's QA member.

Used by the task.close hook right after a work bead closes (no delay) and by cron (safety net for a close
whose actions never ran). It writes nothing: the task.assign hook records the assignee when ATM runs the
assign. A QA bead already assigned is skipped, so a retry or a cron tick never re-sends it.
"""
from atm_bd_types import Bead, BeadStore, ErrorId, Layer, PhaseFile, Refusal, Stack, Stage, Status, TaskAssign, Template


def qa_handoff(phase_id: str, phase: PhaseFile, beads: BeadStore) -> list[TaskAssign] | Refusal:
    if isinstance(ready := beads.ready(phase_id, Stage.QA), Refusal):
        return ready
    assigns: list[TaskAssign] = []
    for qa in ready:  # cache.ready order is priority order
        if qa.status != Status.OPEN or qa.assignee is not None:
            continue
        if isinstance(target := qa_target(qa, beads), Refusal):
            return target
        assigns.append(TaskAssign(task=qa.id, recipient=phase.qa_member, actor=phase.lead,
                                  template=phase.template(Template.QA), review=target[1]))
    return assigns


def qa_target(qa: Bead, beads: BeadStore) -> tuple[Bead, Layer] | Refusal:
    r"""The work bead and layer QA reviews: the lead's `checked_bead` when set, else the highest recorded layer
    among the QA bead's closed blockers (its dev bead, or the latest fix). Layers stacked later are ignored."""
    if isinstance(stack_id := qa.stack_bead(), Refusal):
        return stack_id
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return stack_bead
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return stack
    chosen = qa.meta.checked_bead
    candidates = [chosen] if chosen is not None else [d.id for d in qa.blockers() if d.status == Status.CLOSED]
    best: tuple[int, Bead, Layer] | None = None
    for bead_id in candidates:
        if isinstance(bead := beads.show(bead_id), Refusal):
            return bead
        if isinstance(work := bead.work(), Refusal):
            continue  # a blocker that is not work
        layer, position = stack.layer_for(work.branch), stack.position(work.branch)
        if layer is not None and position is not None and (best is None or position > best[0]):
            best = (position, bead, layer)
    if best is None:
        return Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=qa)
    return best[1], best[2]
```

## Validation
`just lint`, `just test`, `npx pyright -p design/lite-code`.
