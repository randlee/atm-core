"""hooks/task-close.md: one test per Transitions row (T-CLOSE-01..04), per guard, and per Errors row."""
import atm_bd_types as T
from lite_helpers import (DEV, QA, FakePhases, FakeStore, action_template, action_vars, bead, expect_refused,
                      hook_request, layer, reply_actions, reply_data)

S = T.Status
LAYER = layer("sprint/bo-10", "a" * 40, 10, "main")
STACK = bead("stack", stage=T.Stage.STACK, layers={LAYER.branch: LAYER})


def dev_bead(status=S.IN_PROGRESS, branch=LAYER.branch, stage=T.Stage.DEV):
    return bead("bo-10", status, DEV, stage=stage, branch=branch, stack_bead="stack", sprint="sprint-10", phase="bo")


def qa_bead():
    return bead("bo-10-qa", S.IN_PROGRESS, QA, stage=T.Stage.QA, stack_bead="stack", sprint="sprint-10", phase="bo")


def run(hooks, bead_record, req, *extra, phases=None, **store_kw):
    store = FakeStore(bead_record, STACK, *extra, **store_kw)
    return hooks["hook"].hook(req, store, phases or FakePhases()), store


def completed(caller=DEV, task="bo-10", **kw):
    return hook_request(caller=caller, task_id=task, outcome="completed", **kw)


# T-CLOSE-01 GIVE_BACK
def test_t_close_01_refused_reopens_and_returns_refuse_template(hooks):
    req = hook_request(caller=DEV, task_id="bo-10", outcome="refused", close_reason="blocked on X")
    reply, store = run(hooks, dev_bead(), req)
    assert reply_data(reply)["result"] == "handled" and store.writes == [("reopen", "bo-10", None, DEV)]
    [action] = reply_actions(reply)
    assert action_template(action).endswith("/refuse.md")
    assert action_vars(action) == {"reason": "blocked on X"}


def test_t_close_01_cancelled_same_as_refused(hooks):
    req = hook_request(caller=DEV, task_id="bo-10", outcome="cancelled", close_reason="dup")
    reply, store = run(hooks, dev_bead(), req)
    assert reply_data(reply)["result"] == "handled" and store.writes[0][0] == "reopen"


def test_t_close_01_missing_reason(hooks):
    reply, store = run(hooks, dev_bead(), hook_request(caller=DEV, task_id="bo-10", outcome="refused"))
    expect_refused(reply, T.ErrorId.CLOSE_REASON_EMPTY, store)


# T-CLOSE-02 WORK_CLOSE
def test_t_close_02_layer_recorded_closes_with_layer_evidence(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"))
    layer_value = {"kind": "layer", "branch": LAYER.branch, "head": LAYER.head, "pr": LAYER.pr, "base": LAYER.base}
    assert reply_data(reply)["result"] == "handled" and store.writes == [("close", "bo-10", layer_value, DEV)]
    [action] = reply_actions(reply)
    assert action_template(action).endswith("/dev-close.md")
    assert action_vars(action) == {"branch": LAYER.branch, "head": LAYER.head, "pr": LAYER.pr, "base": LAYER.base}


def test_t_close_02_report_missing(hooks):
    reply, store = run(hooks, dev_bead(), completed())
    expect_refused(reply, T.ErrorId.CLOSE_REPORT_NOT_INLINE, store)


def test_t_close_02_layer_not_on_ledger(hooks):
    reply, store = run(hooks, dev_bead(branch="sprint/not-stacked"), completed(close_report="done"))
    expect_refused(reply, T.ErrorId.STACK_NOT_RECORDED, store)
    assert "sprint/not-stacked" in reply_data(reply)["message"]


def test_t_close_02_raced_write(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"), race=True)
    expect_refused(reply, T.ErrorId.BEAD_RACED, store, writes=1)


def test_t_close_02_fix_stage_is_work_close(hooks):
    reply, store = run(hooks, dev_bead(stage=T.Stage.FIX), completed(close_report="done"))
    assert reply_data(reply)["result"] == "handled" and store.writes[0][0] == "close"


# T-CLOSE-03 QA_PASS
def test_t_close_03_pass_closes_with_sha(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="PASS", verdict_sha=LAYER.head)
    reply, store = run(hooks, qa_bead(), req, target=LAYER)
    assert reply_data(reply)["result"] == "handled"
    assert store.writes == [("close", "bo-10-qa", {"kind": "qa_pass", "sha": LAYER.head}, QA)]
    [action] = reply_actions(reply)
    assert action_template(action).endswith("/qa-pass.md")
    assert action_vars(action) == {"sha": LAYER.head}


def test_t_close_03_sha_differs(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="PASS", verdict_sha="b" * 40)
    reply, store = run(hooks, qa_bead(), req, target=LAYER)
    expect_refused(reply, T.ErrorId.QA_NOT_ON_STACK, store)


def test_t_close_03_no_target(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="PASS", verdict_sha=LAYER.head)
    reply, store = run(hooks, qa_bead(), req)
    expect_refused(reply, T.ErrorId.QA_NOTHING_TO_REVIEW, store)


def test_t_close_03_invalid_verdict(hooks):
    reply, store = run(hooks, qa_bead(), completed(caller=QA, task="bo-10-qa"), target=LAYER)
    expect_refused(reply, T.ErrorId.QA_VERDICT, store)


# T-CLOSE-04 QA_FINDINGS
def findings_req(*findings):
    return completed(caller=QA, task="bo-10-qa", verdict="FINDINGS", verdict_sha=LAYER.head,
                     findings=list(findings))


def test_t_close_04_poured_findings_close_qa(hooks):
    findings = [{"id": "f-1", "blocking": True}, {"id": "f-2", "blocking": False}]
    poured = [bead("f-1", blocks_ids=["sprint-10"]), bead("f-2", blocks_ids=["bo"])]
    reply, store = run(hooks, qa_bead(), findings_req(*findings), *poured, target=LAYER)
    assert reply_data(reply)["result"] == "handled"
    assert store.refreshes == [["f-1", "f-2", "sprint-10", "bo"]]
    assert store.writes == [("close", "bo-10-qa", {"kind": "qa_fail", "sha": LAYER.head,
                                                       "findings": findings}, QA)]
    [action] = reply_actions(reply)
    assert action_template(action).endswith("/qa-findings.md")
    assert action_vars(action) == {"sha": LAYER.head, "ids": ["f-1", "f-2"]}


def test_t_close_04_blocking_finding_not_blocking_sprint(hooks):
    finding = {"id": "f-1", "blocking": True}
    reply, store = run(hooks, qa_bead(), findings_req(finding), bead("f-1", blocks_ids=["bo"]), target=LAYER)
    expect_refused(reply, T.ErrorId.FINDINGS_NOT_POURED, store)
    assert "f-1" in reply_data(reply)["message"]


def test_t_close_04_non_blocking_finding_must_block_phase(hooks):
    finding = {"id": "f-2", "blocking": False}
    reply, store = run(hooks, qa_bead(), findings_req(finding), bead("f-2", blocks_ids=["sprint-10"]), target=LAYER)
    expect_refused(reply, T.ErrorId.FINDINGS_NOT_POURED, store)


def test_t_close_04_finding_bead_missing(hooks):
    reply, store = run(hooks, qa_bead(), findings_req({"id": "f-9", "blocking": True}), target=LAYER)
    expect_refused(reply, T.ErrorId.BEAD_NOT_FOUND, store)


def test_t_close_04_sha_differs(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="FINDINGS", verdict_sha="b" * 40)
    reply, store = run(hooks, qa_bead(), req, target=LAYER)
    expect_refused(reply, T.ErrorId.QA_NOT_ON_STACK, store)


# Guards
def test_not_a_phase_task(hooks):
    store = FakeStore(dev_bead())
    reply = hooks["hook"].hook(hook_request(hook="task.close", phase_task=False), store, FakePhases())
    assert reply_data(reply)["result"] == "not-applicable" and store.writes == []


def test_phase_unreadable_and_profile_mismatch(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="x"), phases=FakePhases(readable=False))
    expect_refused(reply, T.ErrorId.PHASE_UNREADABLE, store)
    reply, store = run(hooks, dev_bead(), completed(close_report="x"), phases=FakePhases(profile="orchestration"))
    expect_refused(reply, T.ErrorId.PROFILE_MISMATCH, store)


def test_missing_bead(hooks):
    store = FakeStore(STACK)
    reply = hooks["hook"].hook(completed(task="nope", close_report="x"), store, FakePhases())
    expect_refused(reply, T.ErrorId.BEAD_NOT_FOUND, store)


def test_caller_not_assignee(hooks):
    reply, store = run(hooks, dev_bead(), completed(caller="other", close_report="x"))
    expect_refused(reply, T.ErrorId.TASK_NOT_ASSIGNEE, store)


def test_closed_bead_replays_only_the_close_action(hooks):
    reply, store = run(hooks, dev_bead(S.CLOSED), completed(close_report="done"))
    assert reply_data(reply)["result"] == "handled" and store.writes == []
    assert action_template(reply_actions(reply)[0]).endswith("/dev-close.md")


def test_closed_qa_replays_validated_pass_without_a_write(hooks):
    closed = bead("bo-10-qa", S.CLOSED, QA, stage=T.Stage.QA, stack_bead="stack", sprint="sprint-10", phase="bo")
    req = completed(caller=QA, task="bo-10-qa", verdict="PASS", verdict_sha=LAYER.head)
    reply, store = run(hooks, closed, req, target=LAYER)
    assert reply_data(reply)["result"] == "handled" and store.writes == []
    assert action_template(reply_actions(reply)[0]).endswith("/qa-pass.md")


def test_land_close_requires_merge_sha(hooks):
    landing = bead("land", S.IN_PROGRESS, DEV, stage=T.Stage.LAND)
    report = f'```json\n{{"merged":"{LAYER.head}"}}\n```'
    reply, store = run(hooks, landing, completed(task="land", close_report=report))
    assert reply_data(reply)["result"] == "handled"
    assert store.writes == [("close", "land", {"kind": "merged", "sha": LAYER.head}, DEV)]
    assert action_template(reply_actions(reply)[0]).endswith("/land-close.md")
    reply, store = run(hooks, landing, completed(task="land", close_report="no sha"))
    expect_refused(reply, T.ErrorId.LAND_EVIDENCE, store)


def test_closed_bead_cannot_be_given_back(hooks):
    req = hook_request(caller=DEV, task_id="bo-10", outcome="refused", close_reason="blocked")
    reply, store = run(hooks, dev_bead(S.CLOSED), req)
    expect_refused(reply, T.ErrorId.BEAD_CLOSED, store)


def test_not_started(hooks):
    for status in (S.OPEN, S.BLOCKED, S.DEFERRED):
        reply, store = run(hooks, dev_bead(status), completed(close_report="x"))
        expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)


def test_invalid_outcome_is_request_invalid(hooks):
    reply = hook_request(caller=DEV, task_id="bo-10", outcome="banana")
    assert isinstance(reply, T.Refusal) and reply.id == T.ErrorId.REQUEST_INVALID


def test_close_never_writes_the_ledger_or_assigns(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"))
    assert [write[1] for write in store.writes] == ["bo-10"]
    assert all("assign" not in action_template(action) for action in reply_actions(reply))
