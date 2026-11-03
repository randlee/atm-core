"""cron decide for lite (dev -> qa). Pure: facts in, actions out; the framework executes them.

intent.md, "The close contract": "When the dev bead closes the QA bead is released and is assigned by the
existing `atm task assign` path." Who and with what template: policy.toml params `qa_agent`, `qa_template`.
"""
from atm_bd_types import Action, Assign, AssignVars, Bead, Params, PhaseFacts, Report, Stage, Status


def released_qa(bead: Bead) -> bool:
    """A ready QA bead nobody holds: its dev blocker has closed (bd ready) and it is still open and unassigned."""
    return bead.stage == Stage.QA and bead.status == Status.OPEN and bead.assignee is None


def decide(facts: PhaseFacts, params: Params) -> list[Action]:
    agent, template = params.text("qa_agent"), params.text("qa_template")
    if agent is None or template is None:
        return [Report("cron:params", "config", None, "policy.toml [params] needs text qa_agent and qa_template.")]
    return [Assign(f"assign:{b.id}", agent, b.id, template, AssignVars(b.id, facts.phase, facts.phase_file.profile))
            for b in facts.ready or [] if released_qa(b)]
