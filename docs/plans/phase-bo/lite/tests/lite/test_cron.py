"""Exercise lite decisions with real wheel facts and controlled inputs."""
import json
import subprocess
from pathlib import Path

import atm_bd_types as T
import pytest
from atm_bd_types._native import _stack
from lite_helpers import LEAD, QA, bead, layer, load_hook, phase_file

CRON = load_hook("atm-bd-cron")
PARAMS = T.Params([("qa_agent", QA), ("qa_template", "templates/qa.xml.j2"), ("dev_agent", "dev"),
                   ("dev_template", "templates/dev.xml.j2"), ("lead", LEAD),
                   ("land_template", "templates/land.xml.j2")])
HEAD = "a" * 40


def facts(*ready, tasks=(), checks=T.CheckState.PASSING, head=HEAD, layers=True, repo=True, beads=None, top=None):
    top = top or layer("work", HEAD, 42)
    stack = T.StackFact(bead("stack", stage=T.Stage.STACK), _stack([top] if layers else []), "main", None)
    phase = phase_file(lead=LEAD, repo="example/repo" if repo else None)
    return T.PhaseFacts("bo", 0, phase, tasks=None if tasks is None else list(tasks),
                        beads=[bead("dev", branch="work")] if beads is None else beads,
                        ready=list(ready), stack=stack,
                        top_checks=None if checks is None else T.TopChecks(head, checks))


def test_qa_assignment_has_recorded_target():
    [action] = CRON.decide(facts(bead("qa", stage=T.Stage.QA, checked_bead="dev")), PARAMS)
    assert isinstance(action, T.Assign) and action.to == QA
    assert (action.vars.review.repo, action.vars.review.pr_number, action.vars.review.branch,
            action.vars.review.commit) == ("example/repo", 42, "work", HEAD)


def test_no_assignment_when_task_read_fails_or_task_already_exists():
    qa = bead("qa", stage=T.Stage.QA, checked_bead="dev")
    assert CRON.decide(facts(qa, tasks=None), PARAMS) == []
    assert CRON.decide(facts(qa, tasks=[T.TaskFact("qa", QA, T.TaskState.ACTIVE, "qa")]), PARAMS) == []
    assert CRON.decide(facts(qa, tasks=[T.TaskFact("dev", "dev", T.TaskState.ACTIVE, "dev")]), PARAMS) == []


def test_land_waits_for_open_qa_task_in_same_sprint():
    landing = bead("land", stage=T.Stage.LAND, sprint="sprint")
    qa = bead("qa", stage=T.Stage.QA, sprint="sprint")
    task = T.TaskFact("qa", QA, T.TaskState.ACTIVE, "qa")
    assert CRON.decide(facts(landing, tasks=[task], beads=[qa]), PARAMS) == []


@pytest.mark.parametrize("state,head,expected", [(T.CheckState.PENDING, HEAD, None),
    (T.CheckState.NONE, HEAD, None), (T.CheckState.PASSING, "b" * 40, None),
    (T.CheckState.FAILING, HEAD, "report"), (T.CheckState.PASSING, HEAD, "assign"), (None, HEAD, None)])
def test_land_requires_passing_ci_on_exact_head(state, head, expected):
    actions = CRON.decide(facts(bead("land", stage=T.Stage.LAND), checks=state, head=head), PARAMS)
    if expected is None:
        assert actions == []
    elif expected == "report":
        assert len(actions) == 1 and isinstance(actions[0], T.Report) and actions[0].key == "cron:ci:land"
    else:
        assert len(actions) == 1 and isinstance(actions[0], T.Assign)


@pytest.mark.parametrize("repo,layers", [(False, True), (True, False)])
def test_land_without_target_reports(repo, layers):
    [action] = CRON.decide(facts(bead("land", stage=T.Stage.LAND), repo=repo, layers=layers), PARAMS)
    assert isinstance(action, T.Report) and action.key == "cron:review:land"


@pytest.mark.parametrize("stage", [T.Stage.DEV, T.Stage.QA, T.Stage.LAND])
def test_decision_vars_strictly_render_assignment(tmp_path, stage):
    [action] = CRON.decide(facts(bead("task", stage=stage, checked_bead="dev")), PARAMS)
    values = {"task_id": action.vars.task_id, "phase": action.vars.phase, "profile": action.vars.profile}
    if action.vars.review is not None:
        review = action.vars.review
        values.update(repo=review.repo, pr_number=review.pr_number, branch=review.branch, commit=review.commit)
    var_file = tmp_path / "vars.json"
    var_file.write_text(json.dumps(values))
    template = Path(__file__).resolve().parents[2] / "skills/atm-bd-lite" / action.template
    subprocess.run(["sc-compose", "render", "--root", str(template.parent), "--strict", "--file", str(template), "--var-file", str(var_file)],
                   capture_output=True, text=True, check=True, timeout=10)


@pytest.mark.parametrize("branch,pr", [("", 42), ("work", 0)])
def test_land_missing_branch_or_pr_reports(branch, pr):
    [action] = CRON.decide(facts(bead("land", stage=T.Stage.LAND), top=layer(branch, HEAD, pr)), PARAMS)
    assert isinstance(action, T.Report) and action.key == "cron:review:land"
