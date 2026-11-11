#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.4.0"]
# ///
"""task.assign: hooks/task-assign.md. One write: the assignee."""
TIMEOUT_MS = 2500

import sys
from atm_bd_types import (Hook, BeadPatch, BeadStore, CacheStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli,
                          Refusal)
from atm_bd_types import AssignTransition as Transition


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path, "lite"), Refusal):
        return HookReply.refused(phase)
    if (agent := req.assignee) is None:
        return HookReply.refused(Refusal(ErrorId.REQUEST_INVALID, field="assignee"))
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if req.caller != phase.lead:
        return HookReply.refused(Refusal(ErrorId.ASSIGN_NOT_LEAD, lead=phase.lead))
    if bead.open_blockers:
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead.id, ids=bead.open_blockers))

    match Transition.select(bead.status, bead.assignee, agent):
        case Transition.ASSIGN_SAME:  # T-ASSIGN-01
            return HookReply.pass_through()
        case Transition.ASSIGN:  # T-ASSIGN-02
            patch = BeadPatch.assign(bead, agent)
        case Transition.ASSIGN_CLOSED:  # T-ASSIGN-03
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead.id))
        case Transition.ASSIGN_NOT_READY:  # T-ASSIGN-04
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead.id, status=bead.status.name))

    if (failed := beads.update(patch, actor=req.caller)) is not None:
        return HookReply.refused(failed)
    return HookReply.pass_through()


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json(Hook.TASK_ASSIGN))
    elif isinstance(store := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(store).to_json(Hook.TASK_ASSIGN))
    else:
        print(hook(req, store, PhaseFilesCli()).to_json(Hook.TASK_ASSIGN))
