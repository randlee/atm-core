#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.start hook: the assignee starts an unblocked bead."""
import sys

from atm_bd_types import BdCli, BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli, Refusal, Status

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if not bead.assigned_to(req.caller):
        return HookReply.refused(Refusal(ErrorId.TASK_NOT_ASSIGNEE, bead=bead, caller=req.caller))
    if bead.open_blockers():
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
    match bead.status:
        case Status.OPEN:
            return HookReply.from_result(beads.update(BeadPatch.start(bead), actor=req.caller))
        case Status.IN_PROGRESS:
            return HookReply.pass_through()
        case Status.CLOSED:
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    print((HookReply.refused(req) if isinstance(req, Refusal) else hook(req, BdCli(timeout_seconds=2), PhaseFilesCli())).to_json())
