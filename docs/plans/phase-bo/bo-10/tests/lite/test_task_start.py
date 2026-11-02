"""hooks/task-start.md: one test per Transitions row (T-START-01..04) and per Errors row."""
import fake_atm_bd_types as T
from conftest import DEV, FakePhases, FakeStore, expect_refused

S = T.Status


def run(hooks, bead, caller=DEV, phases=None, **store_kw):
    store = FakeStore(bead, **store_kw)
    return hooks["start-hook"].hook(T.HookRequest(caller=caller, task_id=bead.id), store, phases or FakePhases()), store


# Transitions
def test_t_start_01_open_writes_one_start(hooks):
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee=DEV))
    assert reply.kind == "pass-through" and store.writes == [("start", "b", None, DEV)]


def test_t_start_01_raced_write_is_bead_raced(hooks):
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee=DEV), race=True)
    expect_refused(reply, T.ErrorId.BEAD_RACED, store, writes=1)


def test_t_start_02_in_progress_passes_through(hooks):
    reply, store = run(hooks, T.Bead("b", S.IN_PROGRESS, assignee=DEV))
    assert reply.kind == "pass-through" and store.writes == []


def test_t_start_03_closed(hooks):
    reply, store = run(hooks, T.Bead("b", S.CLOSED, assignee=DEV))
    expect_refused(reply, T.ErrorId.BEAD_CLOSED, store)


def test_t_start_04_blocked_and_deferred(hooks):
    for status in (S.BLOCKED, S.DEFERRED):
        reply, store = run(hooks, T.Bead("b", status, assignee=DEV))
        expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)


# Guards and Errors
def test_not_a_phase_task_is_not_applicable(hooks):
    store = FakeStore(T.Bead("b"))
    reply = hooks["start-hook"].hook(T.HookRequest(phase_task=False), store, FakePhases())
    assert reply.kind == "not-applicable" and store.writes == []


def test_phase_unreadable_and_profile_mismatch(hooks):
    reply, store = run(hooks, T.Bead("b", assignee=DEV), phases=FakePhases(readable=False))
    expect_refused(reply, T.ErrorId.PHASE_UNREADABLE, store)
    reply, store = run(hooks, T.Bead("b", assignee=DEV), phases=FakePhases(profile="orchestration"))
    expect_refused(reply, T.ErrorId.PROFILE_MISMATCH, store)


def test_missing_bead_is_bead_not_found(hooks):
    store = FakeStore()
    reply = hooks["start-hook"].hook(T.HookRequest(caller=DEV, task_id="nope"), store, FakePhases())
    expect_refused(reply, T.ErrorId.BEAD_NOT_FOUND, store)


def test_caller_not_assignee(hooks):
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee="other"))
    expect_refused(reply, T.ErrorId.TASK_NOT_ASSIGNEE, store)
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee=None))
    expect_refused(reply, T.ErrorId.TASK_NOT_ASSIGNEE, store)


def test_open_blockers_before_status(hooks):
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee=DEV, open_blockers=["b-dep"]))
    expect_refused(reply, T.ErrorId.BEAD_NOT_READY, store)


def test_start_never_reads_the_ledger(hooks):
    """The stack bead is absent from the store; start must not need it."""
    reply, store = run(hooks, T.Bead("b", S.OPEN, assignee=DEV, stack_bead="stack-x"))
    assert reply.kind == "pass-through"
