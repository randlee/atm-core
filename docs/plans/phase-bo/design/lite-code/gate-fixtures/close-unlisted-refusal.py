"""GATE FIXTURE, not bo-10 code. ANC-054 question set, expected listed = no for the STACK_HELD refusal.

A task.close hook shaped to pass the gate's deterministic checks (four Transition arms, one beads.update,
stack_bead only read, no subprocess, <= 100 lines). Its WORK_CLOSE arm refuses with STACK_HELD when the
ledger is locked by another agent: a refusal that is neither a row nor an Errors entry of hooks/task-close.md.
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
            if ledger.assignee and ledger.assignee != req.caller:
                return HookReply.refused(Refusal(ErrorId.STACK_HELD, holder=ledger.assignee))
            if (layer := ledger.layer_for(bead.work.branch)) is None:
                return HookReply.refused(Refusal(ErrorId.STACK_NOT_RECORDED, branch=bead.work.branch))
            patch, template, args = BeadPatch.close(bead, evidence=layer), Template.DEV_CLOSE, layer
        case Transition.QA_PASS:  # T-CLOSE-03
            if isinstance(target := beads.show(bead.target_bead()), Refusal):
                return HookReply.refused(target)
            if isinstance(ledger := beads.show(target.stack_bead), Refusal):
                return HookReply.refused(ledger)
            if (head := ledger.head_of(target.work.branch)) != (sha := req.qa_verdict().sha):
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=sha, head=head))
            patch, template, args = BeadPatch.close(bead, evidence=sha), Template.QA_PASS, sha
        case Transition.QA_FINDINGS:  # T-CLOSE-04
            if isinstance(target := beads.show(bead.target_bead()), Refusal):
                return HookReply.refused(target)
            if isinstance(ledger := beads.show(target.stack_bead), Refusal):
                return HookReply.refused(ledger)
            verdict = req.qa_verdict()
            if (head := ledger.head_of(target.work.branch)) != verdict.sha:
                return HookReply.refused(Refusal(ErrorId.QA_NOT_ON_STACK, reviewed=verdict.sha, head=head))
            unpoured = [f for f in verdict.findings
                        if isinstance(poured := beads.show(f.id), Refusal) or not poured.blocks(f.blocker(bead))]
            if unpoured:
                return HookReply.refused(Refusal(ErrorId.FINDINGS_NOT_POURED, ids=unpoured, sprint=bead.sprint))
            patch, template, args = BeadPatch.close(bead, evidence=verdict), Template.QA_FINDINGS, verdict
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
