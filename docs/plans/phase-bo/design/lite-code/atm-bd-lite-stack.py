#!/usr/bin/env -S uv run --quiet --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["atm-bd-types==0.2.0"]
# ///
"""atm-bd-lite stack claim: the dev holds the stack bead while it rebases (bd --claim, as sanity-stack-link).

  acquire --phase P --bead B   claim the stack bead; prints the top to rebase onto (the trunk for a new stack)
  record  --phase P --bead B   check the pushed PR is stacked on the top; record the layer and release the
                               claim in one write guarded by --if-assignee <dev>
  release --bead B             give the stack back without recording (abort); the lead recovers an abandoned
                               claim with bd reclaim, after which the old holder's record is STACK_CLAIM_LOST

Only a live (OPEN/IN_PROGRESS) work bead may claim or record; CLOSED, BLOCKED and DEFERRED refuse.
"""
import sys

from atm_bd_types import (Acquire, BdCli, Bead, BeadPatch, BeadStore, ErrorId, Git, GitCli, Layer, PhaseFile, PhaseFiles,
                          PhaseFilesCli, ProviderCli, PullRequests, Record, Refusal, Release, Stack, StackCommand,
                          StackReply, Status, Work)

PROFILE = "lite"  # policy: this file serves phases whose phase file says profile = "lite" (ANC-026, ANC-061)


def work_bead(bead_id: str, phase: PhaseFile, beads: BeadStore) -> tuple[Bead, Work, str, str] | Refusal:
    r"""A live (OPEN/IN_PROGRESS) work bead, its work, owner and stack bead id. A CLOSED bead never
    writes the stack again: its close-side retry is the task.close hook's recovery from the recorded layer."""
    if isinstance(bead := beads.show(bead_id), Refusal):
        return bead
    match bead.status:
        case Status.OPEN | Status.IN_PROGRESS:  # task.start already refused a blocked bead
            pass
        case Status.CLOSED:
            return Refusal(ErrorId.BEAD_CLOSED, bead=bead, phase=phase)
        case Status.BLOCKED | Status.DEFERRED:
            return Refusal(ErrorId.BEAD_NOT_READY, bead=bead)
    if isinstance(work := bead.work(), Refusal):
        return work
    if isinstance(owner := bead.owner(), Refusal):
        return owner
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return stack_id
    return bead, work, owner, stack_id


def acquire(cmd: Acquire, beads: BeadStore, phases: PhaseFiles) -> StackReply:
    if isinstance(phase := phases.load(cmd.phase_config_path(), PROFILE), Refusal):
        return StackReply.refused(phase)
    if isinstance(found := work_bead(cmd.bead, phase, beads), Refusal):
        return StackReply.refused(found)
    _, work, owner, stack_id = found
    if isinstance(stack_bead := beads.claim(stack_id, owner), Refusal):
        return StackReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        if (released := beads.release(stack_id, owner)) is not None:
            return StackReply.refused(Refusal(ErrorId.STACK_RELEASE_FAILED, stack_bead=stack_id, holder=owner, cause=stack, release=released))
        return StackReply.refused(stack)
    top = stack.top(excluding=work.branch)
    return StackReply.acquired(top=top, first=top == stack.trunk)


def record(cmd: Record, beads: BeadStore, git: Git, prs: PullRequests, phases: PhaseFiles) -> StackReply:
    if isinstance(phase := phases.load(cmd.phase_config_path(), PROFILE), Refusal):
        return StackReply.refused(phase)
    if isinstance(found := work_bead(cmd.bead, phase, beads), Refusal):
        return StackReply.refused(found)
    _, work, owner, stack_id = found
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return StackReply.refused(stack_bead)
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return StackReply.refused(stack)
    top = stack.top(excluding=work.branch)  # the trunk for the first layer
    if isinstance(top_head := git.remote_head(top), Refusal):
        return StackReply.refused(top_head)
    if isinstance(head := git.pushed_head(work), Refusal):
        return StackReply.refused(head)
    if isinstance(pr := prs.open_pr(work.branch, phase.provider), Refusal):
        return StackReply.refused(pr)
    if pr is None:
        return StackReply.refused(Refusal(ErrorId.PR_NOT_FOUND, work=work, top=top))
    if pr.head != head:
        return StackReply.refused(Refusal(ErrorId.HEAD_NOT_PUSHED, work=work, head=head))
    if pr.base != top:
        return StackReply.refused(Refusal(ErrorId.STACK_NOT_STACKED, pr=pr, top=top))
    if isinstance(stacked := git.contains(work, head, top_head), Refusal):
        return StackReply.refused(stacked)
    if not stacked:
        return StackReply.refused(Refusal(ErrorId.STACK_NOT_ANCESTOR, work=work, top=top))
    layer = Layer(base=top, branch=work.branch, head=head, pr=pr.number)
    return StackReply.recorded(beads.update(BeadPatch.record(stack_bead, stack.with_layer(layer), owner), actor=owner), layer)


def release(cmd: Release, beads: BeadStore) -> StackReply:
    if isinstance(bead := beads.show(cmd.bead), Refusal):
        return StackReply.refused(bead)
    if isinstance(owner := bead.owner(), Refusal):
        return StackReply.refused(owner)
    if isinstance(stack_id := bead.stack_bead(), Refusal):
        return StackReply.refused(stack_id)
    return StackReply.released(beads.release(stack_id, owner))


if __name__ == "__main__":
    beads = BdCli(timeout_seconds=10)
    match StackCommand.parse(sys.argv[1:]):
        case Refusal() as refusal:
            reply = StackReply.refused(refusal)
        case Acquire() as cmd:
            reply = acquire(cmd, beads, PhaseFilesCli())
        case Record() as cmd:
            reply = record(cmd, beads, GitCli(timeout_seconds=10), ProviderCli(timeout_seconds=10), PhaseFilesCli())
        case Release() as cmd:
            reply = release(cmd, beads)
    print(reply.to_json())
