#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0", "atm-bd-shared"]
# [tool.uv.sources]
# atm-bd-shared = { path = "../../shared" }  # .atm-bd/atm-bd-lite/scripts -> .atm-bd/shared
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

from atm_bd_shared.qa_handoff import qa_handoff, qa_target

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
