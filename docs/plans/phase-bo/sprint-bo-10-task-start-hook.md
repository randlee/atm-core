# Sprint bo-10: lite task.start hook

Repo: atm-bd-orchestration. Branch `sprint/bo-10-task-start-hook`, stacked on bo-9. Owned paths: `plugins/atm-bd-orchestration/skills/atm-bd-lite/scripts/atm-bd-task-start-hook.py` and its test.

Installs to `.atm-bd/atm-bd-lite/scripts/` (see bo-12, "Install layout").

## Goal
The assignee starts an unblocked bead. The start records the work's base: the stack top at start (the trunk for a new stack), with no claim.

## Rules (all lite sprints)
- The code below is the deliverable. Implement it as written; any change it needs is a plan change and goes to fenix first.
- Pyright strict passes with 0 errors (`design/lite-code/pyrightconfig.json`); no `dict`, `Any`, untyped JSON, inline profile/metadata/branch strings or hand-built paths (ANC-062).
- Tests drive the hook's `hook()`/command function with in-memory fakes of the atm-bd-types traits (`BeadStore`, `PhaseFiles`). Every interleaving is stepped by hand: no sleeps, threads, timing or real bd/dolt/daemon (CLAUDE.md flaky-test rule).
- Every case asserts the exact `HookReply`/`StackReply` JSON, including the `ErrorId` of each refusal, and the exact list of writes the fake received (empty when none).

## Acceptance: one case per arm
- [ ] Not a phase task: not-applicable. Phase file unreadable or another profile: refused `PHASE_UNREADABLE` / `PROFILE_MISMATCH`.
- [ ] Bead missing: `BEAD_NOT_FOUND`. Caller is not the assignee: `TASK_NOT_ASSIGNEE`. The bead has an open blocker: `BEAD_NOT_READY`.
- [ ] OPEN: exactly one write, `BeadPatch.start(snapshot, base)`. If the fake reports exit 13, the reply is refused `BEAD_RACED`.
- [ ] Base (Rand: "there should be a base for the work (current stack top) when the work starts, but no 'claim'"):
  - [ ] a new stack gives base = trunk;
  - [ ] a stack with layers gives base = the top layer's branch;
  - [ ] the stack bead is only read, never claimed or written.
- [ ] A missing `work`, missing `stack_bead` or undecodable stack refuses with the typed error and makes no write.
- [ ] IN_PROGRESS: pass-through with no write. CLOSED: `BEAD_CLOSED`. BLOCKED or DEFERRED: `BEAD_NOT_READY`.
- [ ] Malformed stdin: refused `REQUEST_INVALID` from `HookRequest.parse`.
- [ ] Jev: the file matches the arms above in both directions (ANC-053, 054).

## Code
```python
#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite task.start hook: the assignee starts an unblocked bead; its base is the stack top at start (the trunk
for a new stack), recorded on the bead. No claim: the dev claims the stack only when its work is complete."""
import sys

from atm_bd_types import CacheStore, BeadPatch, BeadStore, ErrorId, HookReply, HookRequest, PhaseFiles, PhaseFilesCli, Refusal, Stack, Status

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
        case Status.OPEN:  # the work's base is the stack top now; no claim (the dev claims only at completion)
            if isinstance(work := bead.work(), Refusal):
                return HookReply.refused(work)
            if isinstance(stack_id := bead.stack_bead(), Refusal):
                return HookReply.refused(stack_id)
            if isinstance(stack_bead := beads.show(stack_id), Refusal):
                return HookReply.refused(stack_bead)
            if isinstance(stack := Stack.decode(stack_bead), Refusal):
                return HookReply.refused(stack)
            return HookReply.from_result(beads.update(BeadPatch.start(bead, base=stack.top(excluding=work.branch)), actor=req.caller))
        case Status.IN_PROGRESS:
            return HookReply.pass_through()
        case Status.CLOSED:
            return HookReply.refused(Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase))
        case Status.BLOCKED | Status.DEFERRED:
            return HookReply.refused(Refusal(ErrorId.BEAD_NOT_READY, bead=bead))


if __name__ == "__main__":
    req = HookRequest.parse(sys.stdin.read())
    if isinstance(req, Refusal):
        print(HookReply.refused(req).to_json())
    elif isinstance(beads := CacheStore.for_phase(req.phase), Refusal):
        print(HookReply.refused(beads).to_json())
    else:
        print(hook(req, beads, PhaseFilesCli()).to_json())
```
