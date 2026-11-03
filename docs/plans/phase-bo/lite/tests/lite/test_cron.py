"""lite cron decide (scripts/atm-bd-cron.py) against fake PhaseFacts: intent.md, "The close contract"."""
import fake_atm_bd_types as T
from conftest import LEAD, QA, load_hook

S = T.Status
CRON = load_hook("atm-bd-cron")
PARAMS = T.Params([("qa_agent", QA), ("qa_template", "templates/qa.md.j2")])


def facts(*ready: T.Bead, read_failed: bool = False) -> T.PhaseFacts:
    return T.PhaseFacts("bo", 0, T.PhaseFile(lead=LEAD), None if read_failed else list(ready))


def test_released_qa_bead_is_assigned_to_qa_agent() -> None:
    assert CRON.decide(facts(T.Bead("q", stage="qa")), PARAMS) == [
        T.Assign("assign:q", QA, "q", "templates/qa.md.j2", T.AssignVars("q", "bo", "lite"))]


def test_held_closed_or_non_qa_beads_are_left_alone() -> None:
    ready = (T.Bead("held", stage="qa", assignee=QA), T.Bead("running", S.IN_PROGRESS, stage="qa"),
             T.Bead("done", S.CLOSED, stage="qa"), T.Bead("dev", stage="dev"))
    assert CRON.decide(facts(*ready), PARAMS) == []


def test_failed_ready_read_decides_nothing() -> None:
    assert CRON.decide(facts(read_failed=True), PARAMS) == []


def test_missing_params_report_instead_of_assigning() -> None:
    reply = CRON.decide(facts(T.Bead("q", stage="qa")), T.Params([("qa_agent", QA)]))
    assert [type(a) for a in reply] == [T.Report] and reply[0].key == "cron:params"
