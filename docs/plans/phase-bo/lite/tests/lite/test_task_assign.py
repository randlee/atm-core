"""hooks/task-assign.md: one test per Transitions row (T-ASSIGN-01..04) and per Errors row."""
import atm_bd_types as T
from lite_helpers import DEV, LEAD, FakePhases, FakeStore, bead, expect_refused, hook_request, reply_data

S = T.Status


def run(hooks, bead, caller=LEAD, assignee=DEV, phases=None, **store_kw):
    store = FakeStore(bead, **store_kw)
    req = hook_request(hook="task.assign", caller=caller, task_id=bead.id, assignee=assignee)
    return hooks["assign-hook"].hook(req, store, phases or FakePhases()), store


# Transitions
def test_t_assign_01_already_this_agent_passes_through(hooks):
    reply, store = run(hooks, bead("b", S.OPEN, assignee=DEV))
    assert reply_data(reply)["result"] == "pass-through" and store.writes == []


def test_t_assign_02_open_unassigned_writes_one_assign(hooks):
    reply, store = run(hooks, bead("b", S.OPEN))
    assert reply_data(reply)["result"] == "pass-through"
    assert store.writes == [("assign", "b", DEV, LEAD)]


def test_t_assign_02_in_progress_other_agent_reassigns(hooks):
    reply, store = run(hooks, bead("b", S.IN_PROGRESS, assignee="other"))
    assert reply_data(reply)["result"] == "pass-through" and store.writes == [("assign", "b", DEV, LEAD)]


def test_t_assign_02_raced_write_is_bead_raced(hooks):
    reply, store = run(hooks, bead("b", S.OPEN), race=True)
    expect_refused(reply, T.ErrorId.BEAD_RACED, store, writes=1)


def test_t_assign_03_closed(hooks):
    reply, store = run(hooks, bead("b", S.CLOSED))
    expect_refused(reply, T.ErrorId.BEAD_CLOSED, store)


def test_t_assign_04_blocked_and_deferred(hooks):
    for status in (S.BLOCKED, S.DEFERRED):
        reply, store = run(hooks, bead("b", status))
        expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)


# Guards and Errors
def test_not_a_phase_task_is_not_applicable(hooks):
    store = FakeStore(bead("b"))
    reply = hooks["assign-hook"].hook(hook_request(hook="task.assign", phase_task=False, assignee=DEV), store, FakePhases())
    assert reply_data(reply)["result"] == "not-applicable" and store.writes == []


def test_phase_unreadable_and_profile_mismatch(hooks):
    reply, store = run(hooks, bead("b"), phases=FakePhases(readable=False))
    expect_refused(reply, T.ErrorId.PHASE_UNREADABLE, store)
    reply, store = run(hooks, bead("b"), phases=FakePhases(profile="orchestration"))
    expect_refused(reply, T.ErrorId.PROFILE_MISMATCH, store)


def test_missing_assignee_is_request_invalid(hooks):
    reply, store = run(hooks, bead("b"), assignee=None)
    expect_refused(reply, T.ErrorId.REQUEST_INVALID, store)


def test_missing_bead_is_bead_not_found(hooks):
    store = FakeStore()
    reply = hooks["assign-hook"].hook(hook_request(hook="task.assign", caller=LEAD, task_id="nope", assignee=DEV), store, FakePhases())
    expect_refused(reply, T.ErrorId.BEAD_NOT_FOUND, store)


def test_caller_not_lead(hooks):
    reply, store = run(hooks, bead("b"), caller=DEV)
    expect_refused(reply, T.ErrorId.ASSIGN_NOT_LEAD, store)


def test_open_blockers_before_status(hooks):
    reply, store = run(hooks, bead("b", S.OPEN, open_blockers=["b-dep"]))
    expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)
    assert "b-dep" in reply_data(reply)["message"]
