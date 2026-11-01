#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.close hook.

Dev/fix close: before closing, the dev recorded its layer on the stack bead under its bd claim
(atm-bd-lite-stack.py acquire -> rebase onto the top (the trunk for a new stack), push -> record; record
releases). Only a live work bead records. This hook only checks the
recorded layer is still what is pushed, closes the bead (guarded) and hands off QA once nothing else
blocks it. A retry of a CLOSED bead recovers from the recorded layer and resumes only the handoff.
QA close: the verdict names the recorded head of the QA bead's checked layer (set by the handoff to the
work bead that just closed; the lead points it higher to direct QA there); layers stacked later are ignored. PASS closes QA recording the sha (a retry of that PASS passes); FAIL
files one fix bead (id from the QA bead and full reviewed sha, create-once) carrying the report and
blocking QA, and dispatches it.
"""
import sys

from atm_bd_types import (BdCli, Bead, BeadMeta, BeadPatch, BeadStore, ErrorId, Git, GitCli, HookAction, HookReply,
                          HookRequest, Layer, NewBead, Outcome, PhaseFile, PhaseFiles, PhaseFilesCli,
                          QaFail, QaPass, Refusal, Stack, Stage, Status, TaskAssign, Template)

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)
FIX_ID = "{qa}-fix-{sha}"  # hook policy: one fix bead per QA bead and FULL reviewed sha; bd create --id is create-once
FIX_BRANCH = "fix/{fix}"  # hook policy: the fix branch is named after its bead (D-5)


def work_close(req: HookRequest, phase: PhaseFile, bead: Bead, beads: BeadStore, git: Git) -> HookReply:
    if isinstance(qa_id := bead.qa_bead(), Refusal):
        return HookReply.refused(qa_id)
    if isinstance(qa := beads.show(qa_id), Refusal):
        return HookReply.refused(qa)
    if isinstance(layer := recorded_layer(phase, bead, beads), Refusal):
        return HookReply.refused(layer)
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:  # fresh close: the recorded layer must still be what is pushed
            if bead.open_blockers():
                return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
            if isinstance(remote := git.remote_head(layer.branch), Refusal):
                return HookReply.refused(remote)
            if remote != layer.head:  # pushed after recording: record again under the lease
                return HookReply.refused(Refusal(ErrorId.STACK_LAYER_MOVED, layer=layer, remote=remote))
            if (failed := beads.update(BeadPatch.close(bead, f"{layer.branch} at {layer.head}; PR #{layer.pr}"), actor=req.caller)) is not None:
                return HookReply.refused(failed)
            return qa_handoff(req, phase, bead, qa, beads, fresh=True)
        case Status.CLOSED:  # recovery: the recorded layer is the proof; the branch may have moved on or merged
            return qa_handoff(req, phase, bead, qa, beads, fresh=False)
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


def recorded_layer(phase: PhaseFile, bead: Bead, beads: BeadStore) -> Layer | Refusal:
    r"""The layer the dev recorded on the stack bead under the lease (atm-bd-lite-stack.py record)."""
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


def qa_handoff(req: HookRequest, phase: PhaseFile, bead: Bead, qa: Bead, beads: BeadStore, fresh: bool) -> HookReply:
    match qa.status:
        case Status.CLOSED:  # QA already finished: let the original close complete, never redispatch
            return HookReply.pass_through()
        case Status.BLOCKED | Status.DEFERRED:  # task.assign would refuse; the lead holds QA
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=qa))
        case Status.OPEN | Status.IN_PROGRESS:
            if qa.open_blockers(ignoring=bead.id):
                return HookReply.pass_through()
            checked = bead.id  # a fresh close points QA at its layer; a retry keeps the target set since (fix, lead)
            if not fresh and not isinstance(current := qa.checked_bead(), Refusal):
                checked = current
            if not qa.assigned_to(phase.qa_member) or qa.checked_bead() != checked:
                if (failed := beads.update(BeadPatch.handoff(qa, phase.qa_member, checked=checked), actor=phase.lead)) is not None:
                    return HookReply.refused(failed)
            assign = TaskAssign(task=qa.id, recipient=phase.qa_member, actor=phase.lead, template=phase.template(Template.QA))
            return HookReply.handled([HookAction.assign(req, assign), HookAction.original(req)])


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
    if isinstance(checked_id := bead.checked_bead(), Refusal):
        return HookReply.refused(checked_id)
    if isinstance(checked := beads.show(checked_id), Refusal):
        return HookReply.refused(checked)
    if isinstance(work := checked.work(), Refusal):
        return HookReply.refused(work)
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return HookReply.refused(stack_id)
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return HookReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return HookReply.refused(stack)
    if (layer := stack.layer_for(work.branch)) is None or layer.head != verdict.sha:  # later layers are ignored
        return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, checked=work.branch))
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
            reason = req.close_reason()
            return HookReply.refused(reason) if isinstance(reason, Refusal) else HookReply.pass_through()
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
