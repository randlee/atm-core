from __future__ import annotations

from pathlib import Path
import subprocess
import sys

import pytest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import bd_cache as bc
import bd_cache_cron as cron
from test_bd_cache import FakeBd, workspace


@pytest.fixture(autouse=True)
def no_beads_dir(monkeypatch):
    monkeypatch.delenv("BEADS_DIR", raising=False)


class Sender:
    def __init__(self):
        self.sent, self.fail = [], 0

    def __call__(self, team, lead, body):
        if self.fail:
            self.fail -= 1
            return False, "atm send failed"
        self.sent.append((team, lead, body))
        return True, ""


@pytest.fixture
def setup(tmp_path):
    root = workspace(tmp_path)
    (root / ".atm-bd").mkdir(exist_ok=True)
    (root / ".atm-bd" / "phase-x.toml").write_text('root = "root-x"\nlead = "lead-x"\n')
    (root / ".atm-bd" / "phase-y.toml").write_text('root = "root-y"\nlead = "lead-y"\n')
    bd = FakeBd()
    bd.add("root-x", labels=["phase-x"])
    bd.add("root-y", labels=["phase-y"], status="closed")
    bd.add("b1", status="in_progress", assignee="dev")
    return root, bd, bc.BdCache(root, runner=bd, clock=lambda: 1792749600.0), Sender()


def run(setup, **custom):
    root, bd, cache, sender = setup
    return cron.tick(root, custom or {"team": "t"}, cache=cache, send_fn=sender)


def escalate(bd, cache):
    cache.write("b1", "update", {"status": "open"}, idem_key="k", actor="san", precondition={"assignee": "dev"})
    bd.rows["b1"]["assignee"] = "other"
    bd.touch("b1")


def test_tick_syncs_only_phases_with_an_open_root(setup):
    root, bd, cache, sender = setup
    assert run(setup) == {"result": "pass-through"}
    synced = [c for c in bd.calls if c[1] == "list" and "-l" in c]
    assert [c[c.index("-l") + 1] for c in synced] == ["phase-x"]
    assert [r["id"] for r in cache.list("y")] == ["root-y"]  # root cached only to read its status


def test_escalation_is_sent_once_to_the_phase_lead(setup):
    root, bd, cache, sender = setup
    escalate(bd, cache)
    assert run(setup) == {"result": "pass-through"}
    assert [(t, lead) for t, lead, _ in sender.sent] == [("t", "lead-x")]
    assert "b1" in sender.sent[0][2] and "resolve 1" in sender.sent[0][2]
    run(setup)
    assert len(sender.sent) == 1


def test_undelivered_escalation_is_retried_and_the_tick_refuses(setup):
    root, bd, cache, sender = setup
    escalate(bd, cache)
    sender.fail = 1
    result = run(setup)
    assert result["result"] == "refused" and "atm send failed" in result["message"]
    assert cache.unannounced_escalations()
    assert run(setup) == {"result": "pass-through"} and len(sender.sent) == 1


def test_sync_failure_is_announced_once_per_cause(setup):
    root, bd, cache, sender = setup
    bd.rows["b1"]["status"] = "nope"  # a reachable bd whose rows break the contract (an outage is test_backend_outage_*)
    for _ in range(2):
        assert run(setup) == {"result": "pass-through"}
    assert len(sender.sent) == 1 and "phase x" in sender.sent[0][2]
    bd.rows["b1"]["status"] = "in_progress"
    run(setup)  # recovery clears the alert
    assert cache.sync_alert("x") is None


def test_missing_team_is_refused(setup):
    root, bd, cache, sender = setup
    assert cron.tick(root, {}, cache=cache, send_fn=sender)["result"] == "refused"


def test_stop_only_when_no_active_phase_and_no_open_work(setup):
    root, bd, cache, sender = setup
    assert cron.should_stop(root, cache) is False  # phase x open
    bd.rows["root-x"]["status"] = "closed"
    bd.touch("root-x")
    cache.sync("x")
    cache.write("b1", "close", {}, idem_key="w", actor="san")
    assert cron.should_stop(root, cache) is False  # pending write
    cache.drain()
    assert cron.should_stop(root, cache) is True


def test_tick_budget_defers_drain_entries(setup):
    root, bd, cache, sender = setup
    cache.write("b1", "close", {}, idem_key="w", actor="san")
    times = iter([0.0] + [100.0] * 20)
    result = cron.tick(root, {"team": "t"}, cache=cache, send_fn=sender, clock=lambda: next(times))
    assert result == {"result": "pass-through"}
    assert cache.db.execute("SELECT state FROM journal").fetchone()["state"] == "pending"


def test_custom_phases_limits_sync_and_stop(setup):
    root, bd, cache, sender = setup
    (root / ".atm-bd" / "phase-z.toml").write_text('root = "root-z"\nlead = "lead-z"\n')
    bd.add("root-z", labels=["phase-z"])
    assert run(setup, team="t", phases=["z"]) == {"result": "pass-through"}
    synced = [c[c.index("-l") + 1] for c in bd.calls if c[1] == "list" and "-l" in c]
    assert synced == ["phase-z"]
    bd.rows["root-z"]["status"] = "closed"
    bd.touch("root-z")
    cache.sync("z")
    assert cron.should_stop(root, cache, only=["z"]) is True
    assert cron.should_stop(root, cache) is False  # phase x still open


def test_malformed_custom_phases_is_refused(setup):
    assert run(setup, team="t", phases="bk")["result"] == "refused"


def down(bd):
    bd.fail[:] = [(lambda c: True, OSError("connection refused"))] * 100


def test_backend_outage_is_announced_once_and_sync_failures_are_not(setup):
    root, bd, cache, sender = setup
    cache.write("b1", "close", {}, idem_key="k", actor="san")
    run(setup)  # roots cached while bd answers
    sender.sent.clear()
    cache.write("b1", "update", {"add_labels": ["y"]}, idem_key="k2", actor="san")
    down(bd)
    for _ in range(bc.MAX_ATTEMPTS + 2):
        assert run(setup) == {"result": "pass-through"}
    assert [(lead, "unreachable" in body) for _, lead, body in sender.sent] == [("lead-x", True)]
    assert cache.status()["escalated"] == []
    bd.fail.clear()
    assert run(setup) == {"result": "pass-through"}
    assert "y" in bd.rows["b1"]["labels"] and cache.outage() is None
    down(bd)
    run(setup)
    assert len(sender.sent) == 2  # a new outage is announced again


def test_outage_with_uncached_roots_still_tells_every_configured_lead(setup):
    root, bd, cache, sender = setup
    down(bd)
    assert run(setup) == {"result": "pass-through"}
    assert sorted(lead for _, lead, _ in sender.sent) == ["lead-x", "lead-y"]


def test_outage_is_not_cleared_while_only_the_backend_check_answers(setup):
    # bd version/schema answer quickly on a loaded host while every list still times out: one announcement.
    root, bd, cache, sender = setup
    run(setup)
    for _ in range(3):
        bd.fail.append((lambda c: c[1] == "list" and "-l" in c, subprocess.TimeoutExpired("bd", 30)))
        run(setup)
    assert len(sender.sent) == 1 and cache.outage() is not None


def test_undelivered_outage_announcement_is_retried(setup):
    root, bd, cache, sender = setup
    run(setup)
    down(bd)
    sender.fail = 1
    assert run(setup)["result"] == "refused"
    assert cache.outage()["announced_at"] is None
    assert run(setup) == {"result": "pass-through"} and len(sender.sent) == 1


@pytest.mark.parametrize("code, delivered", [(0, True), (1, False)])
def test_send_runs_atm_send_as_atm_bd_at_tool_and_reports_delivery(monkeypatch, code, delivered):
    # cron@tool is atm's daemon-owned scheduler principal and refuses ordinary sends; the module sends as the
    # installer's atm-bd@tool member, and a refused send stays undelivered.
    calls = []

    def run(argv, **kwargs):
        calls.append((argv, kwargs))
        return subprocess.CompletedProcess(argv, code, "", "" if code == 0 else "refused")

    monkeypatch.setattr(cron.subprocess, "run", run)
    monkeypatch.setenv("ATM_IDENTITY", "someone")
    monkeypatch.setenv("ATM_TEAM", "elsewhere")
    ok, _detail = cron.send("dev-team", "lead", "body")
    assert ok is delivered
    [(argv, kwargs)] = calls
    assert argv == [cron.ATM, "send", "lead@dev-team", "--team", "tool", "--stdin", "--json"]
    assert kwargs["input"] == "body"
    assert (kwargs["env"]["ATM_IDENTITY"], kwargs["env"]["ATM_TEAM"]) == ("atm-bd", "tool")
