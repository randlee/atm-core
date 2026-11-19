#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.4.0"]
# ///
"""task.start: hooks/task-start.md. One write: in_progress. Does not read the ledger."""
TIMEOUT_MS = 2500

import sys
from atm_bd_types import (Hook, BeadPatch, BeadStore, CacheStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli,
                          Refusal)
from atm_bd_types import StartTransition as Transition


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path, "lite"), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if bead.assignee != req.caller:
        return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead.id, agent=bead.assignee or ""))
    if bead.open_blockers:
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead.id, ids=bead.open_blockers))

    match Transition.select(bead.status):
        case Transition.START:  # T-START-01
            patch = BeadPatch.start(bead)
        case Transition.START_AGAIN:  # T-START-02
            return HookReply.pass_through()
        case Transition.START_CLOSED:  # T-START-03
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead.id))
        case Transition.START_NOT_READY:  # T-START-04
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead.id, status=bead.status.name))

    if (failed := beads.update(patch, actor=req.caller)) is not None:
        return HookReply.refused(failed)
    return HookReply.pass_through()


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json(Hook.TASK_START))
    elif isinstance(store := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(store).to_json(Hook.TASK_START))
    else:
        print(hook(req, store, PhaseFilesCli()).to_json(Hook.TASK_START))
