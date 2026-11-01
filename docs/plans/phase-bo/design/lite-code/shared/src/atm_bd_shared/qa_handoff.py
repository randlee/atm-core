"""Shared (.atm-bd/shared, package atm-bd-shared): hand ready QA beads to the phase's QA member.

Used by the task.close hook right after a work bead closes (no delay) and by cron (safety net for a close
whose actions never ran). It writes nothing: the task.assign hook records the assignee when ATM runs the
assign. A QA bead already assigned is skipped, so a retry or a cron tick never re-sends it.
"""
from atm_bd_types import Bead, BeadStore, ErrorId, Layer, PhaseFile, Refusal, Stack, Stage, Status, TaskAssign, Template


def qa_handoff(phase_id: str, phase: PhaseFile, beads: BeadStore) -> list[TaskAssign] | Refusal:
    if isinstance(ready := beads.ready(phase_id, Stage.QA), Refusal):
        return ready
    assigns: list[TaskAssign] = []
    for qa in ready:  # bd ready order is priority order
        if qa.status != Status.OPEN or qa.assignee is not None:
            continue
        if isinstance(target := qa_target(qa, beads), Refusal):
            return target
        assigns.append(TaskAssign(task=qa.id, recipient=phase.qa_member, actor=phase.lead,
                                  template=phase.template(Template.QA), review=target[1]))
    return assigns


def qa_target(qa: Bead, beads: BeadStore) -> tuple[Bead, Layer] | Refusal:
    r"""The work bead and layer QA reviews: the lead's `checked_bead` when set, else the highest recorded layer
    among the QA bead's closed blockers (its dev bead, or the latest fix). Layers stacked later are ignored."""
    if isinstance(stack_id := qa.stack_bead(), Refusal):
        return stack_id
    if isinstance(stack_bead := beads.show(stack_id), Refusal):
        return stack_bead
    if isinstance(stack := Stack.decode(stack_bead), Refusal):
        return stack
    chosen = qa.meta.checked_bead
    candidates = [chosen] if chosen is not None else [d.id for d in qa.blockers() if d.status == Status.CLOSED]
    best: tuple[int, Bead, Layer] | None = None
    for bead_id in candidates:
        if isinstance(bead := beads.show(bead_id), Refusal):
            return bead
        if isinstance(work := bead.work(), Refusal):
            continue  # a blocker that is not work
        layer, position = stack.layer_for(work.branch), stack.position(work.branch)
        if layer is not None and position is not None and (best is None or position > best[0]):
            best = (position, bead, layer)
    if best is None:
        return Refusal(ErrorId.QA_NOTHING_TO_REVIEW, bead=qa)
    return best[1], best[2]
