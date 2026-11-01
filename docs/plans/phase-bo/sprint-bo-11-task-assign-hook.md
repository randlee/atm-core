# Sprint bo-11: lite task.assign hook

Repo: atm-bd-orchestration. Branch `sprint/bo-11-task-assign-hook`, stacked on bo-9. Owned paths: `plugins/atm-bd-orchestration/skills/atm-bd-lite/scripts/atm-bd-task-assign-hook.py` and its test.

## Goal
The lead assigns a ready bead, and bd records the assignee.

## Rules (all lite sprints)
- The code below is the deliverable. Implement it as written; any change it needs is a plan change and goes to fenix first.
- Pyright strict passes with 0 errors (`design/lite-code/pyrightconfig.json`); no `dict`, `Any`, untyped JSON, inline profile/metadata/branch strings or hand-built paths (ANC-062).
- Tests drive the hook's `hook()`/command function with in-memory fakes of the atm-bd-types traits (`BeadStore`, `Git`, `PullRequests`, `PhaseFiles`). Every interleaving is stepped by hand: no sleeps, threads, timing or real bd/dolt/daemon (CLAUDE.md flaky-test rule).
- Every case asserts the exact `HookReply`/`StackReply` JSON, including the `ErrorId` of each refusal, and the exact list of writes the fake received (empty when none).

## Acceptance: one case per arm
- [ ] Not a phase task: not-applicable. Phase file unreadable or another profile: refused. Missing assignee: `REQUEST_INVALID`.
- [ ] Caller is not `phase.lead`: `ASSIGN_NOT_LEAD`. The bead has an open blocker: `BEAD_NOT_READY`.
- [ ] OPEN or IN_PROGRESS already assigned to the agent: pass-through with no write. The ATM same-agent re-assign is a logged update (#1619).
- [ ] OPEN or IN_PROGRESS assigned to someone else or nobody: exactly one `BeadPatch.assign(snapshot, agent)`. Exit 13 gives `BEAD_RACED`.
- [ ] CLOSED: `BEAD_CLOSED`. BLOCKED or DEFERRED: `BEAD_NOT_READY`.
- [ ] The QA and fix assignments the close hook returns (actor = lead) are admitted by this hook. This test uses the actual `TaskAssign` the bo-12 fakes produce.
- [ ] Jev: the file matches the arms above in both directions.

## Code
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.assign hook: the lead assigns a ready bead, and bd records the assignee."""
import sys

from atm_bd_types import BdCli, BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli, Refusal, Status

PROFILE = "lite"  # hook policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def hook(req: HookRequest, beads: BeadStore, phases: PhaseFiles) -> HookReply:
    if not req.is_phase_task():
        return HookReply.not_applicable()
    if isinstance(phase := phases.load(req.phase_config_path(), PROFILE), Refusal):
        return HookReply.refused(phase)
    if isinstance(agent := req.assignee, Refusal):
        return HookReply.refused(agent)
    if isinstance(bead := beads.show(req.task_id), Refusal):
        return HookReply.refused(bead)
    if req.caller != phase.lead:
        return HookReply.refused(Refusal(ErrorId.ASSIGN_NOT_LEAD, bead=bead, phase=phase))
    if bead.open_blockers():
        return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:
            if bead.assigned_to(agent):
                return HookReply.pass_through()
            return HookReply.from_result(beads.update(BeadPatch.assign(bead, agent), actor=req.caller))
        case Status.CLOSED:
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    print((HookReply.refused(req) if isinstance(req, Refusal) else hook(req, BdCli(timeout_seconds=2), PhaseFilesCli())).to_json())
```
