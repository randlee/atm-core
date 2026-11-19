"""cron decide for lite (dev -> qa). Pure: facts in, actions out; the framework executes them.

intent.md, "The close contract": "When the dev bead closes the QA bead is released and is assigned by the
existing `atm task assign` path." Who and with what template: policy.toml params `qa_agent`, `qa_template`.
"""
TIMEOUT_MS = 2500

from atm_bd_types import Action, Assign, AssignVars, CheckState, Params, PhaseFacts, Report, ReviewTarget, Stage, Status


def decide(facts: PhaseFacts, params: Params) -> list[Action]:
    if facts.tasks is None:
        return []  # do not assign when the ATM task list is unavailable
    existing = {task.task_id for task in facts.tasks}
    actions: list[Action] = []
    for bead in facts.ready or []:
        if bead.id in existing or bead.status != Status.OPEN or bead.stage not in (Stage.DEV, Stage.QA, Stage.LAND):
            continue
        if bead.stage == Stage.QA and bead.checked_bead in existing:
            continue  # bead close may have journaled before its ATM close action failed
        if bead.stage == Stage.LAND:
            if facts.beads is None:
                continue  # cannot establish that QA tasks have finished
            if any(item.stage == Stage.QA and item.sprint == bead.sprint and item.id in existing
                   for item in facts.beads):
                continue
        if bead.stage == Stage.QA:
            agent_key, template_key = "qa_agent", "qa_template"
        elif bead.stage == Stage.DEV:
            agent_key, template_key = "dev_agent", "dev_template"
        else:
            agent_key, template_key = "lead", "land_template"
        agent, template = params.text(agent_key), params.text(template_key)
        if not agent or not template:
            actions.append(Report(f"cron:params:{bead.id}", "config", bead.id,
                                  f"policy.toml requires {agent_key} and {template_key}."))
            continue
        if bead.stage == Stage.DEV:
            actions.append(Assign(f"assign:{bead.id}", agent, bead.id, template,
                                  AssignVars(bead.id, facts.phase, facts.phase_file.profile)))
            continue
        repo = facts.phase_file.key("repo")
        layers = [] if facts.stack is None else facts.stack.stack.layers
        dev = next((b for b in facts.beads or [] if b.id == bead.checked_bead), None)
        layer = (layers[-1] if layers and bead.stage == Stage.LAND else
                 next((item for item in layers if dev is not None and item.branch == dev.branch), None))
        if not repo or layer is None or not layer.pr or not layer.branch or not layer.head:
            actions.append(Report(f"cron:review:{bead.id}", "config", bead.id,
                                  "Missing repo, checked dev bead, or recorded review layer (branch, PR, head)."))
            continue
        if bead.stage == Stage.LAND:
            checks = facts.top_checks
            if checks is None or checks.head != layer.head:
                continue
            if checks.state == CheckState.FAILING:
                actions.append(Report(f"cron:ci:{bead.id}", "ci", bead.id,
                                      f"Top {layer.head} CI failing; fix on the stack top."))
                continue
            if checks.state != CheckState.PASSING:
                continue
        review = ReviewTarget(repo, layer.pr, layer.branch, layer.head)
        actions.append(Assign(f"assign:{bead.id}", agent, bead.id, template,
                              AssignVars(bead.id, facts.phase, facts.phase_file.profile, review)))
    return actions
