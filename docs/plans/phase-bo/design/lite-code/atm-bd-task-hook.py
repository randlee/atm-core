#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0", "atm-bd-shared"]
# [tool.uv.sources]
# atm-bd-shared = { path = "../../shared" }  # .atm-bd/atm-bd-lite/scripts -> .atm-bd/shared
# ///
"""atm-bd-lite task.close hook. Implements docs/task-close.md, nothing else.

Work bead (dev/fix): the recorded layer exists -> close the bead, assign ready QA beads.
QA bead: the verdict sha is the target layer's head -> PASS closes QA; FAIL creates a fix bead for the dev.
Refused or cancelled: the bead goes back to open, unassigned.
"""
import sys
from atm_bd_shared.qa_handoff import qa_handoff, qa_target
from atm_bd_types import (BdCli, Bead, BeadMeta, BeadPatch, BeadStore, ErrorId, HookAction, HookReply, HookRequest,
                          Layer, NewBead, Outcome, PhaseFile, PhaseFiles, PhaseFilesCli, QaFail, QaPass, Refusal,
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
    if isinstance(layer := recorded_layer(bead, beads), Refusal):  # T-CLOSE-02
        return HookReply.refused(layer)
    if (failed := beads.update(BeadPatch.close(bead, f"{layer.branch} at {layer.head}; PR #{layer.pr}"),
                               actor=req.caller)) is not None:
        return HookReply.refused(failed)
    if isinstance(assigns := qa_handoff(req.phase, phase, beads), Refusal):  # T-CLOSE-03
        return HookReply.refused(assigns)
    return HookReply.handled([HookAction.assign(req, a) for a in assigns] + [HookAction.original(req)])


def recorded_layer(bead: Bead, beads: BeadStore) -> Layer | Refusal:
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
    if isinstance(target := qa_target(bead, beads), Refusal):
        return HookReply.refused(target)
    checked, layer = target
    if layer.head != verdict.sha:  # T-CLOSE-04
        return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, checked=layer.branch))
    match verdict:
        case QaPass():  # T-CLOSE-05
            return HookReply.from_result(
                beads.update(BeadPatch.close(bead, f"PASS at {verdict.sha}", reviewed=verdict.sha), actor=req.caller))
        case QaFail(report=report):  # T-CLOSE-06
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
    reply = HookReply.refused(req) if isinstance(req, Refusal) else hook(req, BdCli(timeout_seconds=2), PhaseFilesCli())
    print(reply.to_json())
