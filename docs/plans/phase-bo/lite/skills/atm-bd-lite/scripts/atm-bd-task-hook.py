#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""task.close: hooks/task-close.md. One write, to the caller's own bead."""
import sys
from atm_bd_types import (BeadPatch, BeadStore, CacheStore, ErrorId, HookAction, HookReply, HookRequest, PhaseFiles,
                          PhaseFilesCli, Refusal, Status, Template)
from atm_bd_types import CloseTransition as Transition


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path, "lite"), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if bead.assignee != req.caller:
        return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead.id, agent=bead.assignee or ""))
    if bead.status != Status.IN_PROGRESS:
        closed = bead.status == Status.CLOSED
        return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED if closed else ErrorId.BEAD_NOT_READY, bead=bead.id))
    if isinstance(transition := Transition.select(req, bead), Refusal):
        return HookReply.refused(transition)

    match transition:
        case Transition.GIVE_BACK:  # T-CLOSE-01
            if not (reason := req.close_reason):
                return HookReply.refused(Refusal(ErrorId.CLOSE_REASON_EMPTY, bead=bead.id))
            patch, action = BeadPatch.reopen(bead), HookAction.close(phase.template(Template.REFUSE), reason)
        case Transition.WORK_CLOSE:  # T-CLOSE-02
            if not req.close_report:
                return HookReply.refused(Refusal(ErrorId.CLOSE_REPORT_NOT_INLINE, bead=bead.id))
            if isinstance(stack := beads.show(bead.stack_bead), Refusal):
                return HookReply.refused(stack)
            if (layer := stack.layer_for(bead.branch)) is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=bead.branch))
            patch, action = BeadPatch.close(bead, layer), HookAction.close(phase.template(Template.DEV_CLOSE), layer)
        case Transition.QA_PASS:  # T-CLOSE-03
            if isinstance(verdict := req.qa_pass(), Refusal):
                return HookReply.refused(verdict)
            if isinstance(target := beads.target_layer(bead), Refusal):
                return HookReply.refused(target)
            if verdict.sha != target.head:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, sha=verdict.sha, branch=target.branch, head=target.head))
            patch, action = BeadPatch.close(bead, verdict), HookAction.close(phase.template(Template.QA_PASS), verdict)
        case Transition.QA_FINDINGS:  # T-CLOSE-04
            if isinstance(verdict := req.qa_findings(), Refusal):
                return HookReply.refused(verdict)
            if isinstance(target := beads.target_layer(bead), Refusal):
                return HookReply.refused(target)
            if verdict.sha != target.head:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, sha=verdict.sha, branch=target.branch, head=target.head))
            for finding in verdict.findings:  # each poured finding bead exists and blocks the sprint (blocking) or phase bead
                if isinstance(poured := beads.show(finding.id), Refusal):
                    return HookReply.refused(poured)
                if not poured.blocks(bead.sprint if finding.blocking else bead.phase):
                    return HookReply.refused(Refusal(ErrorId.FINDINGS_NOT_POURED, ids=[finding.id], sprint=bead.sprint))
            patch, action = BeadPatch.close(bead, verdict), HookAction.close(phase.template(Template.QA_FINDINGS), verdict)

    if (failed := beads.update(patch, actor=req.caller)) is not None:
        return HookReply.refused(failed)
    return HookReply.handled([action])


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(store := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(store).to_json())
    else:
        print(hook(req, store, PhaseFilesCli()).to_json())
