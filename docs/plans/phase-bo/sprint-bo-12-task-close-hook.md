# Sprint bo-12: lite task.close hook (stacks the dev's layer)

Repo: atm-bd-orchestration. Branch `sprint/bo-12-task-close-hook`, stacked on bo-9. Owned paths: `plugins/atm-bd-orchestration/skills/atm-bd-lite/scripts/atm-bd-task-hook.py`, `plugins/atm-bd-orchestration/shared/` (the `atm-bd-shared` package, installed to `.atm-bd/shared/`), and their tests.

## Goal
- **Dev workflow** (Rand): rebase onto the stack top (the trunk on a new stack), push, open the PR, then `atm task close` with the report ending in one fenced JSON layer block (Rand: "that should be fenced json"). The close hook claims the stack bead, checks the block's base is the top, records the layer and releases, in cache writes only (ANC-063).
- **QA** (Rand): QA runs on a stacked layer, and layers stacked later are ignored. The lead may send QA to a higher layer by updating the QA bead's existing `checked_bead`.
- A FAIL verdict files one fix bead that carries the whole report and dispatches it. **BLOCKED:** beads-cache has no `create` op, and a closed ATM task is terminal, so the dev bead cannot be reused. Needs Rand's decision (beads-cache `create` op upstream, or another shape).

## Install layout (Rand)
- Lite scripts install to `.atm-bd/atm-bd-lite/scripts/`: the three task hooks and `atm-bd-lite-cron.py`.
- Shared code installs to `.atm-bd/shared/` as the `atm-bd-shared` package.
- Each lite script's uv header names it with `atm-bd-shared = { path = "../../shared" }`. uv resolves that against the script's own directory, so the working directory does not matter (checked 2026-11-01: 0.49 s on a fresh directory).
- [ ] A test runs the installed close hook from an unrelated working directory and imports `atm_bd_shared`.

## Rules (all lite sprints)
- The code below is the deliverable. Implement it as written; any change it needs is a plan change and goes to fenix first.
- Pyright strict passes with 0 errors (`design/lite-code/pyrightconfig.json`); no `dict`, `Any`, untyped JSON, inline profile/metadata/branch strings or hand-built paths (ANC-062).
- Tests drive the hook's `hook()`/command function with in-memory fakes of the atm-bd-types traits (`BeadStore`, `PhaseFiles`). Every interleaving is stepped by hand: no sleeps, threads, timing or real bd/dolt/daemon (CLAUDE.md flaky-test rule).
- Every case asserts the exact `HookReply` JSON, including the `ErrorId` of each refusal, and the exact list of writes the fake received (empty when none).

## Acceptance: task.close

The hook implements `docs/task-close.md` (§5 of the lite state transitions), and nothing else. There is one test per transition row, against a fake `BeadStore`:
- [ ] T-CLOSE-01: refused or cancelled → one `BeadPatch.reopen` (open, unassigned); the task closes.
- [ ] T-CLOSE-02: dev bead whose report has no valid fenced layer block → refused `LAYER_BLOCK_MISSING`, no write.
- [ ] T-CLOSE-03: block's base is not the stack top → claim, then release, then refused `STACK_NOT_STACKED` naming the top. The stack bead is held by another close → refused `STACK_HELD`. Neither records anything.
- [ ] T-CLOSE-04: block's base is the top (the trunk on a new stack) → claim, then ONE `BeadPatch.record` (layer + release), then one `BeadPatch.close`, then one assign per ready QA bead, then the original close.
- [ ] T-CLOSE-05: QA verdict sha is not the target head → refused `QA_NOT_ON_STACK`, no write.
- [ ] T-CLOSE-06: QA PASS → one `BeadPatch.close(reviewed=sha)`.
- [ ] T-CLOSE-07: QA FAIL → **BLOCKED** on a beads-cache create path (see Goal).
- [ ] Every write goes through the fake `CacheStore`. The grep for `bd ` subprocess calls and `BdCli` in the lite scripts is empty (ANC-063).
- [ ] Each error in the doc's error table: its code, its owner, and a message containing the exact fix command.
- [ ] Jev: `jev-check` on `task-close.toml` passes every T-CLOSE row and every code branch.

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
        case QaFail(report=report):  # T-CLOSE-07 (BLOCKED: beads-cache has no create op)
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

## Code: shared qa_handoff (`.atm-bd/shared`, package `atm-bd-shared`, module `atm_bd_shared.qa_handoff`)
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

