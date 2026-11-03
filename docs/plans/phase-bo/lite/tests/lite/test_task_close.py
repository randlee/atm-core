"""hooks/task-close.md: one test per Transitions row (T-CLOSE-01..04), per guard, and per Errors row."""
import fake_atm_bd_types as T
from conftest import DEV, QA, FakePhases, FakeStore, expect_refused

S = T.Status
LAYER = T.Layer(branch="sprint/bo-10", head="a" * 40)
STACK = T.Bead("stack", layers={LAYER.branch: LAYER})


def dev_bead(status=S.IN_PROGRESS, stack=True):
    return T.Bead("bo-10", status, assignee=DEV, branch=LAYER.branch, stack_bead="stack", sprint="sprint-10", phase="bo")


def qa_bead():
    return T.Bead("bo-10-qa", S.IN_PROGRESS, assignee=QA, stage="qa", stack_bead="stack", sprint="sprint-10", phase="bo")


def run(hooks, bead, req, *extra, phases=None, **store_kw):
    store = FakeStore(bead, STACK, *extra, **store_kw)
    return hooks["hook"].hook(req, store, phases or FakePhases()), store


def completed(caller=DEV, task="bo-10", **kw):
    return T.HookRequest(caller=caller, task_id=task, outcome="completed", **kw)


# T-CLOSE-01 GIVE_BACK
def test_t_close_01_refused_reopens_and_returns_refuse_template(hooks):
    req = T.HookRequest(caller=DEV, task_id="bo-10", outcome="refused", close_reason="blocked on X")
    reply, store = run(hooks, dev_bead(), req)
    assert reply.kind == "handled" and store.writes == [("reopen", "bo-10", None, DEV)]
    assert reply.actions == (T.HookAction("templates/refuse.md", "blocked on X"),)


def test_t_close_01_cancelled_same_as_refused(hooks):
    req = T.HookRequest(caller=DEV, task_id="bo-10", outcome="cancelled", close_reason="dup")
    reply, store = run(hooks, dev_bead(), req)
    assert reply.kind == "handled" and store.writes[0][0] == "reopen"


def test_t_close_01_missing_reason(hooks):
    reply, store = run(hooks, dev_bead(), T.HookRequest(caller=DEV, task_id="bo-10", outcome="refused"))
    expect_refused(reply, T.ErrorId.CLOSE_REASON_EMPTY, store)


# T-CLOSE-02 WORK_CLOSE
def test_t_close_02_layer_recorded_closes_with_layer_evidence(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"))
    assert reply.kind == "handled" and store.writes == [("close", "bo-10", LAYER, DEV)]
    assert reply.actions == (T.HookAction("templates/dev-close.md", LAYER),)


def test_t_close_02_report_missing(hooks):
    reply, store = run(hooks, dev_bead(), completed())
    expect_refused(reply, T.ErrorId.CLOSE_REPORT_NOT_INLINE, store)


def test_t_close_02_layer_not_on_ledger(hooks):
    bead = dev_bead()
    bead.branch = "sprint/not-stacked"
    reply, store = run(hooks, bead, completed(close_report="done"))
    expect_refused(reply, T.ErrorId.STACK_NOT_RECORDED, store)
    assert reply.refusal is not None and reply.refusal.context["branch"] == "sprint/not-stacked"


def test_t_close_02_raced_write(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"), race=True)
    expect_refused(reply, T.ErrorId.BEAD_RACED, store, writes=1)


def test_t_close_02_fix_stage_is_work_close(hooks):
    bead = dev_bead()
    bead.stage = "fix"
    reply, store = run(hooks, bead, completed(close_report="done"))
    assert reply.kind == "handled" and store.writes[0][0] == "close"


# T-CLOSE-03 QA_PASS
def test_t_close_03_pass_closes_with_sha(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="PASS", verdict_sha=LAYER.head)
    reply, store = run(hooks, qa_bead(), req, target=LAYER)
    assert reply.kind == "handled" and store.writes == [("close", "bo-10-qa", T.QaPass(LAYER.head), QA)]
    assert reply.actions == (T.HookAction("templates/qa-pass.md", T.QaPass(LAYER.head)),)


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
    return completed(caller=QA, task="bo-10-qa", verdict="FINDINGS", verdict_sha=LAYER.head, findings=list(findings))


def test_t_close_04_poured_findings_close_qa(hooks):
    blocking, minor = T.Finding("f-1", True), T.Finding("f-2", False)
    poured = [T.Bead("f-1", blocks_ids=["sprint-10"]), T.Bead("f-2", blocks_ids=["bo"])]
    reply, store = run(hooks, qa_bead(), findings_req(blocking, minor), *poured, target=LAYER)
    assert reply.kind == "handled"
    assert store.writes == [("close", "bo-10-qa", T.QaFail(LAYER.head, [blocking, minor]), QA)]
    assert reply.actions[0].template == "templates/qa-findings.md"


def test_t_close_04_blocking_finding_not_blocking_sprint(hooks):
    blocking = T.Finding("f-1", True)
    reply, store = run(hooks, qa_bead(), findings_req(blocking), T.Bead("f-1", blocks_ids=["bo"]), target=LAYER)
    expect_refused(reply, T.ErrorId.FINDINGS_NOT_POURED, store)
    assert reply.refusal is not None and reply.refusal.context["ids"] == ["f-1"]


def test_t_close_04_non_blocking_finding_must_block_phase(hooks):
    minor = T.Finding("f-2", False)
    reply, store = run(hooks, qa_bead(), findings_req(minor), T.Bead("f-2", blocks_ids=["sprint-10"]), target=LAYER)
    expect_refused(reply, T.ErrorId.FINDINGS_NOT_POURED, store)


def test_t_close_04_finding_bead_missing(hooks):
    reply, store = run(hooks, qa_bead(), findings_req(T.Finding("f-9", True)), target=LAYER)
    expect_refused(reply, T.ErrorId.BEAD_NOT_FOUND, store)


def test_t_close_04_sha_differs(hooks):
    req = completed(caller=QA, task="bo-10-qa", verdict="FINDINGS", verdict_sha="b" * 40)
    reply, store = run(hooks, qa_bead(), req, target=LAYER)
    expect_refused(reply, T.ErrorId.QA_NOT_ON_STACK, store)


# Guards
def test_not_a_phase_task(hooks):
    store = FakeStore(dev_bead())
    reply = hooks["hook"].hook(T.HookRequest(phase_task=False), store, FakePhases())
    assert reply.kind == "not-applicable" and store.writes == []


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


def test_status_closed_and_not_started(hooks):
    reply, store = run(hooks, dev_bead(S.CLOSED), completed(close_report="x"))
    expect_refused(reply, T.ErrorId.BEAD_CLOSED, store)
    for status in (S.OPEN, S.BLOCKED, S.DEFERRED):
        reply, store = run(hooks, dev_bead(status), completed(close_report="x"))
        expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)


def test_invalid_outcome_is_request_invalid(hooks):
    reply, store = run(hooks, dev_bead(), T.HookRequest(caller=DEV, task_id="bo-10", outcome="banana"))
    expect_refused(reply, T.ErrorId.REQUEST_INVALID, store)


def test_close_never_writes_the_ledger_or_assigns(hooks):
    reply, store = run(hooks, dev_bead(), completed(close_report="done"))
    assert [w[1] for w in store.writes] == ["bo-10"]          # only the caller's bead
    assert all(a.template != "assign" for a in reply.actions)  # no QA assign action
