#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.4.0"]
# ///
"""task.close: hooks/task-close.md. One write, to the caller's own bead."""
TIMEOUT_MS = 2500

import sys
from atm_bd_types import (Hook, BeadPatch, BeadStore, CacheStore, ErrorId, HookAction, HookReply, HookRequest, PhaseFiles,
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
    if bead.status not in (Status.IN_PROGRESS, Status.CLOSED):
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead.id))
    if isinstance(transition := Transition.select(req, bead), Refusal):
        return HookReply.refused(transition)
    if bead.status == Status.CLOSED and transition == Transition.GIVE_BACK:
        return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead.id))
    match transition:
        case Transition.GIVE_BACK:  # T-CLOSE-01
            if not (reason := req.close_reason):
                return HookReply.refused(Refusal(ErrorId.CLOSE_REASON_EMPTY, bead=bead.id))
            patch, action = BeadPatch.reopen(bead), HookAction.close(req, phase, phase.template(Template.REFUSE), reason)
        case Transition.WORK_CLOSE:  # T-CLOSE-02
            if not req.close_report:
                return HookReply.refused(Refusal(ErrorId.CLOSE_REPORT_NOT_INLINE, bead=bead.id))
            if isinstance(stack := beads.show(bead.stack_bead), Refusal):
                return HookReply.refused(stack)
            if (layer := stack.layer_for(bead.branch)) is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=bead.branch or ""))
            patch, action = BeadPatch.close(bead, layer), HookAction.close(req, phase, phase.template(Template.DEV_CLOSE), layer)
        case Transition.QA_PASS:  # T-CLOSE-03
            if isinstance(verdict := req.qa_pass(), Refusal):
                return HookReply.refused(verdict)
            if bead.checked_bead is None:
                return HookReply.refused(Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=bead.id, reason="checked_bead missing"))
            if isinstance(checked := beads.show(bead.checked_bead), Refusal):
                return HookReply.refused(checked)
            if isinstance(target := beads.target_layer(checked), Refusal):
                return HookReply.refused(target)
            if target.branch is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=""))
            if verdict.sha != target.head:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, sha=verdict.sha, branch=target.branch, head=target.head))
            patch, action = BeadPatch.close(bead, verdict), HookAction.close(req, phase, phase.template(Template.QA_PASS), verdict)
        case Transition.QA_FINDINGS:  # T-CLOSE-04
            if isinstance(verdict := req.qa_findings(), Refusal):
                return HookReply.refused(verdict)
            if bead.checked_bead is None:
                return HookReply.refused(Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=bead.id, reason="checked_bead missing"))
            if isinstance(checked := beads.show(bead.checked_bead), Refusal):
                return HookReply.refused(checked)
            if isinstance(target := beads.target_layer(checked), Refusal):
                return HookReply.refused(target)
            if target.branch is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=""))
            if verdict.sha != target.head:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, sha=verdict.sha, branch=target.branch, head=target.head))
            ids = [finding.id for finding in verdict.findings] + [bead.sprint]
            if any(not finding.blocking for finding in verdict.findings):
                ids.append(bead.phase)
            if (failed := beads.refresh(list(dict.fromkeys(ids)))) is not None:
                return HookReply.refused(failed)
            for finding in verdict.findings:  # each poured finding bead exists and blocks the sprint (blocking) or phase bead
                if isinstance(poured := beads.show(finding.id), Refusal):
                    return HookReply.refused(poured)
                if not poured.blocks(bead.sprint if finding.blocking else bead.phase):
                    return HookReply.refused(Refusal(ErrorId.FINDINGS_NOT_POURED, ids=[finding.id], sprint=bead.sprint or ""))
            patch, action = BeadPatch.close(bead, verdict), HookAction.close(req, phase, phase.template(Template.QA_FINDINGS), verdict)
        case Transition.LAND:  # T-CLOSE-05
            if isinstance(merged := req.merged(), Refusal):
                return HookReply.refused(merged)
            patch, action = BeadPatch.close(bead, merged), HookAction.close(req, phase, phase.template(Template.LAND_CLOSE), merged)

    if bead.status == Status.CLOSED:
        return HookReply.handled([action])
    if (failed := beads.update(patch, actor=req.caller)) is not None:
        return HookReply.refused(failed)
    return HookReply.handled([action])


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json(Hook.TASK_CLOSE))
    elif isinstance(store := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(store).to_json(Hook.TASK_CLOSE))
    else:
        print(hook(req, store, PhaseFilesCli()).to_json(Hook.TASK_CLOSE))
