"""Lite task.close hook (hooks/task-close.md).
"""
import sys
from atm_bd_types import (BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli,
                          CacheStore, Refusal, Status, Template, Transition)


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), "lite"), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if isinstance(transition := Transition.for_close(req.outcome, bead.stage, req.qa_verdict()), Refusal):
        return HookReply.refused(transition)
    if not bead.assigned_to(req.caller):
        return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead, caller=req.caller))
    if bead.status is Status.CLOSED:
        return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead))
    if bead.status is not Status.IN_PROGRESS:
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
    match transition:
        case Transition.GIVE_BACK:  # T-CLOSE-01
            if isinstance(reason := req.close_reason(), Refusal):
                return HookReply.refused(reason)
            patch, template, args = BeadPatch.reopen(bead), Template.REFUSE, reason
        case Transition.WORK_CLOSE:  # T-CLOSE-02
            if isinstance(report := req.close_report(), Refusal):
                return HookReply.refused(report)
            if isinstance(ledger := beads.show(bead.stack_bead), Refusal):
                return HookReply.refused(ledger)
            if (layer := ledger.layer_for(bead.work.branch)) is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=bead.work.branch))
            patch = BeadPatch.close(bead, evidence=layer).also(BeadPatch.record(ledger, ledger.with_layer(layer)))
            template, args = Template.DEV_CLOSE, layer
        case Transition.QA_PASS:  # T-CLOSE-03
            if (target_id := bead.checked_bead or bead.closed_work_blocker()) is None:
                return HookReply.refused(Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=bead))
            if isinstance(target := beads.show(target_id), Refusal):
                return HookReply.refused(target)
            if isinstance(ledger := beads.show(target.stack_bead), Refusal):
                return HookReply.refused(ledger)
            if isinstance(verdict := req.qa_verdict(), Refusal):
                return HookReply.refused(verdict)
            if (head := ledger.head_of(target.work.branch)) != (sha := verdict.sha):
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=sha, head=head))
            patch, template, args = BeadPatch.close(bead, evidence=sha), Template.QA_PASS, sha
        case Transition.QA_FINDINGS:  # T-CLOSE-04
            if (target_id := bead.checked_bead or bead.closed_work_blocker()) is None:
                return HookReply.refused(Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=bead))
            if isinstance(target := beads.show(target_id), Refusal):
                return HookReply.refused(target)
            if isinstance(ledger := beads.show(target.stack_bead), Refusal):
                return HookReply.refused(ledger)
            if isinstance(verdict := req.qa_verdict(), Refusal):
                return HookReply.refused(verdict)
            if (head := ledger.head_of(target.work.branch)) != verdict.sha:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, head=head))
            unpoured = [f for f in verdict.findings if isinstance(poured := beads.show(f.id), Refusal)
                        or not poured.blocks(bead.sprint if f.blocking else bead.phase_bead)]
            if unpoured:
                return HookReply.refused(Refusal(ErrorId.FINDINGS_NOT_POURED, ids=[f.id for f in unpoured], sprint=bead.sprint,
                                                 phase=bead.phase_bead))
            ids = [f.id for f in verdict.findings]
            patch, template, args = BeadPatch.close(bead, evidence=(verdict.sha, ids)), Template.QA_FINDINGS, (verdict.sha, ids)
    if (failed := beads.update(patch, actor=req.caller)) is not None:
        return HookReply.refused(failed)
    return HookReply.handled(phase.close_action(req, template, args))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(beads := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(beads).to_json())
    else:
        print(hook(req, beads, PhaseFilesCli()).to_json())
