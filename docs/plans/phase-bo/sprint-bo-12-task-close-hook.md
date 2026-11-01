# Sprint bo-12: lite task.close hook and the dev's stack claim command

Repo: atm-bd-orchestration. Branch `sprint/bo-12-task-close-hook`, stacked on bo-9. Owned paths: `plugins/atm-bd-orchestration/skills/atm-bd-lite/scripts/atm-bd-task-hook.py`, `.../atm-bd-lite-stack.py`, `plugins/atm-bd-orchestration/shared/scripts/qa_handoff.py` (installed to `.atm-bd/shared/scripts/`), and their tests.

## Goal
- **Dev workflow** (Rand): obtain the stack bead, rebase, update the stack bead, release it, close the task. On a new stack, rebase onto the trunk.
- **QA** (Rand): QA runs on a stacked layer, and layers stacked later are ignored. The lead may send QA to a higher layer by updating the QA bead's existing `checked_bead`.
- A FAIL verdict files one fix bead that carries the whole report and dispatches it.

## Rules (all lite sprints)
- The code below is the deliverable. Implement it as written; any change it needs is a plan change and goes to fenix first.
- Pyright strict passes with 0 errors (`design/lite-code/pyrightconfig.json`); no `dict`, `Any`, untyped JSON, inline profile/metadata/branch strings or hand-built paths (ANC-062).
- Tests drive the hook's `hook()`/command function with in-memory fakes of the atm-bd-types traits (`BeadStore`, `Git`, `PullRequests`, `PhaseFiles`). Every interleaving is stepped by hand: no sleeps, threads, timing or real bd/dolt/daemon (CLAUDE.md flaky-test rule).
- Every case asserts the exact `HookReply`/`StackReply` JSON, including the `ErrorId` of each refusal, and the exact list of writes the fake received (empty when none).

## Acceptance: stack command (`atm-bd-lite-stack.py`)
- [ ] acquire on a new stack (trunk only): claims; reply `top = trunk`, `first = true`. On a stack with layers: `top` = the last layer other than the bead's own.
- [ ] acquire while another agent holds the claim: `STACK_HELD`, naming the holder, with no write.
- [ ] acquire or record with a CLOSED work bead: `BEAD_CLOSED`, no stack write. BLOCKED or DEFERRED: `BEAD_NOT_READY`.
- [ ] acquire where `Stack.decode` fails: a release is attempted. If it succeeds, the decode refusal is returned. If it times out or returns `STACK_CLAIM_LOST`, the reply is `STACK_RELEASE_FAILED`, carrying the cause, the release refusal, the holder and the stack bead.
- [ ] record, first layer: PR base = trunk passes; a non-trunk base gives `STACK_NOT_STACKED`. Second layer: base = layer 1's branch.
- [ ] record refusals, one case each: no PR, `PR_NOT_FOUND`; PR head is not the pushed head, `HEAD_NOT_PUSHED`; dirty worktree, `HEAD_DIRTY`; top not an ancestor, `STACK_NOT_ANCESTOR`. Each makes no write.
- [ ] record after the claim was reclaimed by the lead (`bd reclaim`): `STACK_CLAIM_LOST`, no write.
- [ ] record success: exactly ONE `BeadPatch.record` write (layers plus release). A later acquire by the next bead sees `top` = this layer.
- [ ] release by the owner: released. Release when not held: `STACK_CLAIM_LOST`.

## Acceptance: task.close (work beads)
- [ ] REFUSED or CANCELLED (Rand: the bead "should probably revert to open"), for both work and QA beads: an empty reason gives `CLOSE_REASON_EMPTY`.
  - [ ] An OPEN or IN_PROGRESS bead owned by the caller gets ONE `BeadPatch.reopen` (open, unassigned). It is then re-assignable to another dev through task.assign and task.start.
  - [ ] The give-back makes no stack-bead read or write.
  - [ ] A retry after the bead was given back, or after it was re-assigned to someone else, passes through with no write. CLOSED, BLOCKED and DEFERRED pass through. COMPLETED: assignee check, inline report (`CLOSE_REPORT_NOT_INLINE`), stage (`BEAD_STAGE_MISSING`/`AMBIGUOUS`).
- [ ] No recorded layer: `STACK_NOT_RECORDED`. Remote head differs from the recorded head: `STACK_LAYER_MOVED`, no write.
- [ ] Fresh close: one guarded `BeadPatch.close`. If two closes of the same bead are stepped by hand, one gets `BEAD_RACED` and its retry takes recovery.
- [ ] Recovery (CLOSED bead) in each of these states, with no PR or tip checks:
  - [ ] S+A+B with A retried;
  - [ ] the same, with A's PR merged.
- [ ] QA dispatch: after the close write, the hook calls the shared `qa_handoff` (`.atm-bd/shared/scripts/qa_handoff.py`, which cron also uses).
  - [ ] No QA bead ready: pass-through.
  - [ ] A QA bead that is ready, open and unassigned: handled with [assign to `qa_member`, actor = lead, QA template, review = target layer, then the original].
  - [ ] Several ready: one assign each, in bd ready order.
  - [ ] Already assigned: skipped.
- [ ] A recovery retry after its assign ran passes through with only the original close. No QA bead is re-sent.

## Acceptance: task.close (QA bead)
- [ ] `qa_target`: the lead's `checked_bead` when set; otherwise the highest recorded layer among the QA bead's closed blockers. That is the dev layer, or after a FAIL the latest fix layer. Layers stacked later are ignored, and so is a blocker that is not work.
  - [ ] A verdict on the target's head passes.
  - [ ] An unrequested higher layer's sha gives `QA_NOT_ON_STACK`; once the lead sets `checked_bead` to that layer, the verdict passes.
  - [ ] No reviewable layer gives `QA_NOTHING_TO_REVIEW`.
- [ ] PASS: one `BeadPatch.close(reviewed=sha)`. A retry of the same PASS on the CLOSED QA bead passes through. A PASS(H') or FAIL on that CLOSED bead gives `QA_VERDICT_CONFLICT`.
- [ ] FAIL: the fix id uses the FULL sha. Two shas sharing a 12-character prefix give two fix ids.
- [ ] FAIL: the created fix carries `stack_bead`, `qa_bead`, `reviewed` and the sibling work. Then run the actual created fix DTO through acquire, record and close on the same stack id; a hand-built fix is not allowed here.
- [ ] FAIL when the fix id already exists (`BEAD_EXISTS`), one case per mismatch: wrong parent, owner, stack_bead, reviewed or missing blocks edge each give `BEAD_ID_CONFLICT`.
- [ ] FAIL with an existing matching fix, one case per status: CLOSED gives pass-through (no redispatch); OPEN or IN_PROGRESS is dispatched to the dev; BLOCKED or DEFERRED gives `BEAD_NOT_READY`.
- [ ] The FAIL(H) replay after the fix recorded H2: the matching FAIL(H) recovers without redispatch.
- [ ] Jev: each file matches its arms in both directions.

## Code: task.close hook
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.close hook.

Dev/fix close: the dev claimed the stack bead at completion, rebased onto the top, recorded its layer and
released (atm-bd-lite-stack.py). This hook checks the recorded layer is still what is pushed, closes the bead
(guarded), then hands ready QA beads to the QA member with the shared qa_handoff (cron runs the same method).
A retry of a CLOSED bead recovers from the recorded layer. QA close: the verdict names the head of the QA
bead's target layer (the lead's checked_bead, else the highest layer among its closed blockers); layers
stacked later are ignored. PASS closes QA recording the sha (a retry of that PASS passes); FAIL files one fix
bead (id from the QA bead and full reviewed sha, create-once) carrying the report, and dispatches it.
A refused or cancelled close gives the bead back: open and unassigned.
"""
import sys

from qa_handoff import qa_handoff, qa_target  # .atm-bd/shared/scripts

from atm_bd_types import (BdCli, Bead, BeadMeta, BeadPatch, BeadStore, ErrorId, Git, GitCli, HookAction, HookReply,
                          HookRequest, Layer, NewBead, Outcome, PhaseFile, PhaseFiles, PhaseFilesCli,
                          QaFail, QaPass, Refusal, Stack, Stage, Status, TaskAssign, Template)

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)
FIX_ID = "{qa}-fix-{sha}"  # hook policy: one fix bead per QA bead and FULL reviewed sha; bd create --id is create-once
FIX_BRANCH = "fix/{fix}"  # hook policy: the fix branch is named after its bead (D-5)


def work_close(req: HookRequest, phase: PhaseFile, bead: Bead, beads: BeadStore, git: Git) -> HookReply:
    if isinstance(layer := recorded_layer(bead, beads), Refusal):
        return HookReply.refused(layer)
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:  # fresh close: the recorded layer must still be what is pushed
            if bead.open_blockers():
                return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
            if isinstance(remote := git.remote_head(layer.branch), Refusal):
                return HookReply.refused(remote)
            if remote != layer.head:  # pushed after recording: record again
                return HookReply.refused(Refusal(ErrorId.STACK_LAYER_MOVED, layer=layer, remote=remote))
            if (failed := beads.update(BeadPatch.close(bead, f"{layer.branch} at {layer.head}; PR #{layer.pr}"), actor=req.caller)) is not None:
                return HookReply.refused(failed)
            return dispatch_qa(req, phase, beads)
        case Status.CLOSED:  # recovery: the recorded layer is the proof; the branch may have moved on or merged
            return dispatch_qa(req, phase, beads)
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


def dispatch_qa(req: HookRequest, phase: PhaseFile, beads: BeadStore) -> HookReply:
    if isinstance(assigns := qa_handoff(req.phase, phase, beads), Refusal):
        return HookReply.refused(assigns)
    if not assigns:
        return HookReply.pass_through()
    return HookReply.handled([HookAction.assign(req, assign) for assign in assigns] + [HookAction.original(req)])


def recorded_layer(bead: Bead, beads: BeadStore) -> Layer | Refusal:
    r"""The layer the dev recorded on the stack bead under its claim (atm-bd-lite-stack.py record)."""
    if isinstance(work := bead.work(), Refusal):
        return work
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return stack_id
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return stack_bead
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return stack
    if (layer := stack.layer_for(work.branch)) is None:
        return Refusal(ErrorId.STACK_NOT_RECORDED, bead=bead, stack_bead=stack_bead)
    return layer


def qa_close(req: HookRequest, phase: PhaseFile, bead: Bead, beads: BeadStore) -> HookReply:
    if isinstance(verdict := req.qa_verdict(), Refusal):
        return HookReply.refused(verdict)
    match bead.status:
        case Status.CLOSED:  # only PASS closes QA: a retry of that same PASS passes, anything else conflicts
            if isinstance(recorded := bead.reviewed(), Refusal):
                return HookReply.refused(recorded)
            if isinstance(verdict, QaPass) and recorded == verdict.sha:
                return HookReply.pass_through()
            return HookReply.refused(Refusal(ErrorId.QA_VERDICT_CONFLICT, bead=bead, recorded=recorded, reviewed=verdict.sha))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
        case Status.OPEN | Status.IN_PROGRESS:
            return qa_verdict(req, phase, bead, verdict, beads)


def qa_verdict(req: HookRequest, phase: PhaseFile, bead: Bead, verdict: QaPass | QaFail, beads: BeadStore) -> HookReply:
    if isinstance(target := qa_target(bead, beads), Refusal):
        return HookReply.refused(target)
    checked, layer = target
    if layer.head != verdict.sha:  # QA reviews its target layer; layers stacked later are ignored
        return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, checked=layer.branch))
    if isinstance(work := checked.work(), Refusal):
        return HookReply.refused(work)
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return HookReply.refused(stack_id)
    match verdict:
        case QaPass():
            return HookReply.from_result(beads.update(BeadPatch.close(bead, f"PASS at {verdict.sha}", reviewed=verdict.sha), actor=req.caller))
        case QaFail(report=report):
            if isinstance(dev := checked.owner(), Refusal):
                return HookReply.refused(dev)
            fix_id = FIX_ID.format(qa=bead.id, sha=verdict.sha)
            meta = BeadMeta(work=work.sibling(FIX_BRANCH.format(fix=fix_id)), stack_bead=stack_id, qa_bead=bead.id,
                            reviewed=verdict.sha)
            new = NewBead(id=fix_id, title=f"fix {bead.id} at {verdict.sha.short()}", description=report, parent=bead.id,
                          phase=req.phase, stage=Stage.FIX, assignee=dev, meta=meta, blocks=bead.id)
            if (created := beads.create(new, actor=req.caller)) is not None and created.id != ErrorId.BEAD_EXISTS:
                return HookReply.refused(created)
            if isinstance(fix := beads.show(fix_id), Refusal):
                return HookReply.refused(fix)
            if not new.made(fix):  # an existing id must be this verdict's fix (creation identity, not later fields)
                return HookReply.refused(Refusal(ErrorId.BEAD_ID_CONFLICT, existing=fix, wanted=new))
            match fix.status:
                case Status.CLOSED:  # this verdict was fully processed by an earlier run
                    return HookReply.pass_through()
                case Status.OPEN | Status.IN_PROGRESS:  # (re)dispatch; same-agent re-assign is a logged update
                    assign = TaskAssign(task=fix.id, recipient=dev, actor=phase.lead, template=phase.template(Template.FIX))
                    return HookReply.handled([HookAction.assign(req, assign), HookAction.original(req)])
                case Status.BLOCKED | Status.DEFERRED:  # task.assign would refuse these
                    return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=fix))


def give_back(req: HookRequest, bead: Bead, beads: BeadStore, reason: str) -> HookReply:
    r"""A refused or cancelled task gives its bead back (Rand): open and unassigned, so the lead can assign it
    to any dev. No stack claim exists here: the dev claims the stack only when its work is complete."""
    if not bead.assigned_to(req.caller):  # already given back or re-assigned
        return HookReply.pass_through()
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:
            return HookReply.from_result(beads.update(BeadPatch.reopen(bead, reason), actor=req.caller))
        case Status.CLOSED | Status.BLOCKED | Status.DEFERRED:
            return HookReply.pass_through()


def hook(req: HookRequest, beads: BeadStore, git: Git, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(outcome := req.outcome, Refusal):
        return HookReply.refused(outcome)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    match outcome:
        case Outcome.REFUSED | Outcome.CANCELLED:
            if isinstance(reason := req.close_reason(), Refusal):
                return HookReply.refused(reason)
            return give_back(req, bead, beads, reason)
        case Outcome.COMPLETED:
            if not bead.assigned_to(req.caller):
                return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead, caller=req.caller))
            if isinstance(report := req.close_report(), Refusal):
                return HookReply.refused(report)
            if isinstance(stage := bead.stage, Refusal):
                return HookReply.refused(stage)
            match stage:
                case Stage.DEV | Stage.FIX:
                    return work_close(req, phase, bead, beads, git)
                case Stage.QA:
                    return qa_close(req, phase, bead, beads)


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        reply = HookReply.refused(req)
    else:
        reply = hook(req, BdCli(timeout_seconds=2), GitCli(timeout_seconds=2), PhaseFilesCli())
    print(reply.to_json())
```

## Code: shared qa_handoff (`.atm-bd/shared/scripts/qa_handoff.py`)
```python
"""Shared (.atm-bd/shared/scripts): hand ready QA beads to the phase's QA member.

Used by the task.close hook right after a work bead closes (no delay) and by cron (safety net for a close
whose actions never ran). It writes nothing: the task.assign hook records the assignee when ATM runs the
assign. A QA bead already assigned is skipped, so a retry or a cron tick never re-sends it.
"""
from atm_bd_types import Bead, BeadStore, ErrorId, Layer, PhaseFile, Refusal, Stack, Stage, Status, TaskAssign, Template


def qa_handoff(phase_id: str, phase: PhaseFile, beads: BeadStore) -> list[TaskAssign] | Refusal:
    if isinstance(ready := beads.ready(phase_id, Stage.QA), Refusal):
        return ready
    assigns: list[TaskAssign] = []
    for qa in ready:  # bd ready order is priority order
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
## Code: stack claim command
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite stack claim: the dev holds the stack bead while it rebases (bd --claim, as sanity-stack-link).

  acquire --phase P --bead B   claim the stack bead; prints the top to rebase onto (the trunk for a new stack)
  record  --phase P --bead B   check the pushed PR is stacked on the top; record the layer and release the
                               claim in one write guarded by --if-assignee <dev>
  release --bead B             give the stack back without recording (abort); the lead recovers an abandoned
                               claim with bd reclaim, after which the old holder's record is STACK_CLAIM_LOST

Only a live (OPEN/IN_PROGRESS) work bead may claim or record; CLOSED, BLOCKED and DEFERRED refuse.
"""
import sys

from atm_bd_types import (Acquire, BdCli, Bead, BeadPatch, BeadStore, ErrorId, Git, GitCli, Layer, PhaseFile, PhaseFiles,
                          PhaseFilesCli, ProviderCli, PullRequests, Record, Refusal, Release, Stack, StackCommand,
                          StackReply, Status, Work)

PROFILE = "lite"  # policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def work_bead(bead_id: str, phase: PhaseFile, beads: BeadStore) -> tuple[Bead, Work, str, str] | Refusal:
    r"""A live (OPEN/IN_PROGRESS) work bead, its work, owner and stack bead id. A CLOSED bead never
    writes the stack again: its close-side retry is the task.close hook's recovery from the recorded layer."""
    if isinstance(bead := beads.show(bead_id), Refusal):
        return bead
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:  # task.start already refused a blocked bead
            pass
        case Status.CLOSED:
            return Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase)
        case Status.BLOCKED | Status.DEFERRED:
            return Refusal(ErrorId.BEAD_NOT_READY, bead=bead)
    if isinstance(work := bead.work(), Refusal):
        return work
    if isinstance(owner := bead.owner(), Refusal):
        return owner
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return stack_id
    return bead, work, owner, stack_id


def acquire(cmd: Acquire, beads: BeadStore, phases: PhaseFiles) -> StackReply:
    if isinstance(phase := phases.load(cmd.phase_config_path(), PROFILE), Refusal):
        return StackReply.refused(phase)
    if isinstance(found := work_bead(cmd.bead, phase, beads), Refusal):
        return StackReply.refused(found)
    _, work, owner, stack_id = found
    if isinstance(stack_bead := beads.claim(stack_id, owner), Refusal):
        return StackReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        if (released := beads.release(stack_id, owner)) is not None:
            return StackReply.refused(Refusal(ErrorId.STACK_RELEASE_FAILED, stack_bead=stack_id, holder=owner, cause=stack, release=released))
        return StackReply.refused(stack)
    top = stack.top(excluding=work.branch)
    return StackReply.acquired(top=top, first=top == stack.trunk)


def record(cmd: Record, beads: BeadStore, git: Git, prs: PullRequests, phases: PhaseFiles) -> StackReply:
    if isinstance(phase := phases.load(cmd.phase_config_path(), PROFILE), Refusal):
        return StackReply.refused(phase)
    if isinstance(found := work_bead(cmd.bead, phase, beads), Refusal):
        return StackReply.refused(found)
    _, work, owner, stack_id = found
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return StackReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return StackReply.refused(stack)
    top = stack.top(excluding=work.branch)  # the trunk for the first layer
    if isinstance(top_head := git.remote_head(top), Refusal):
        return StackReply.refused(top_head)
    if isinstance(head := git.pushed_head(work), Refusal):
        return StackReply.refused(head)
    if isinstance(pr := prs.open_pr(work.branch, phase.provider), Refusal):
        return StackReply.refused(pr)
    if pr is None:
        return StackReply.refused(Refusal(ErrorId.PR_NOT_FOUND, work=work, top=top))
    if pr.head != head:
        return StackReply.refused(Refusal(ErrorId.HEAD_NOT_PUSHED, work=work, head=head))
    if pr.base != top:
        return StackReply.refused(Refusal(ErrorId.STACK_NOT_STACKED, pr=pr, top=top))
    if isinstance(stacked := git.contains(work, head, top_head), Refusal):
        return StackReply.refused(stacked)
    if not stacked:
        return StackReply.refused(Refusal(ErrorId.STACK_NOT_ANCESTOR, work=work, top=top))
    layer = Layer(base=top, branch=work.branch, head=head, pr=pr.number)
    return StackReply.recorded(beads.update(BeadPatch.record(stack_bead, stack.with_layer(layer), owner), actor=owner), layer)


def release(cmd: Release, beads: BeadStore) -> StackReply:
    if isinstance(bead := beads.show(cmd.bead), Refusal):
        return StackReply.refused(bead)
    if isinstance(owner := bead.owner(), Refusal):
        return StackReply.refused(owner)
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return StackReply.refused(stack_id)
    return StackReply.released(beads.release(stack_id, owner))


if __name__ == "__main__":
    beads = BdCli(timeout_seconds=10)
    match StackCommand.parse(sys.argv[1:]):
        case Refusal() as refusal:
            reply = StackReply.refused(refusal)
        case Acquire() as cmd:
            reply = acquire(cmd, beads, PhaseFilesCli())
        case Record() as cmd:
            reply = record(cmd, beads, GitCli(timeout_seconds=10), ProviderCli(timeout_seconds=10), PhaseFilesCli())
        case Release() as cmd:
            reply = release(cmd, beads)
    print(reply.to_json())
```
