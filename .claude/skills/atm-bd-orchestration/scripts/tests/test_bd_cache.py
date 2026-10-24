from __future__ import annotations

import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys

import pytest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
import bd_cache as bc
import bd_models

FIXTURES = Path(__file__).resolve().parent / "fixtures" / "bd-1.3.1"
SCHEMA_TEXT = (FIXTURES / "bd-schema.json").read_text()


class FakeBd:
    """Deterministic bd: rows in `bd list --json` shape, scripted failures, recorded calls."""

    def __init__(self):
        self.rows = {}
        self.calls = []
        self.fail = []  # (predicate, outcome) consumed in order; outcome = returncode int or exception
        self.tick = 0
        self.schema = SCHEMA_TEXT

    def stamp(self):
        self.tick += 1
        return f"2026-10-23T10:{self.tick // 60:02d}:{self.tick % 60:02d}Z"

    def add(self, bead, **fields):
        row = {"id": bead, "title": bead, "status": "open", "priority": 2, "issue_type": "task",
               "labels": ["phase-x"], "metadata": {}, "created_at": "2026-10-23T09:00:00Z",
               "dependency_count": 0, "dependent_count": 0, "comment_count": 0, "updated_at": self.stamp()}
        row.update(fields)
        self.rows[bead] = row
        return row

    def touch(self, bead):
        self.rows[bead]["updated_at"] = self.stamp()

    def edge(self, bead, target, kind):
        self.rows[bead].setdefault("dependencies", []).append(
            {"issue_id": bead, "depends_on_id": target, "type": kind, "created_at": self.stamp(), "metadata": "{}"})

    def done(self, command, code=0, out=""):
        return subprocess.CompletedProcess(command, code, out, "" if code == 0 else "fake failure")

    def __call__(self, command, **kwargs):
        self.calls.append(command)
        for index, (predicate, outcome) in enumerate(self.fail):
            if predicate(command):
                del self.fail[index]
                if isinstance(outcome, BaseException):
                    raise outcome
                return self.done(command, outcome)
        verb = command[1]
        if verb == "version":
            return self.done(command, out="bd version 1.3.1 (dev: fake)\n")
        if verb == "schema":
            return self.done(command, out=self.schema)
        if verb == "list":
            if "--id" in command:
                wanted = command[command.index("--id") + 1].split(",")
                out = [self.rows[i] for i in wanted if i in self.rows]
            else:
                label = command[command.index("-l") + 1]
                since = command[command.index("--updated-after") + 1] if "--updated-after" in command else ""
                out = [r for r in self.rows.values() if label in r["labels"] and r["updated_at"] > since]
            return self.done(command, out=json.dumps(out))
        bead = command[3] if verb == "dep" else command[2]
        row = self.rows.get(bead)
        if row is None:
            return self.done(command, 1)
        if verb == "close":
            row["status"] = "closed"
            if "--reason" in command:
                row["close_reason"] = command[command.index("--reason") + 1]
        elif verb == "dep":
            self.edge(bead, command[4], command[command.index("--type") + 1])
        elif verb == "update":
            flags = list(zip(command[3::2], command[4::2]))
            for flag, value in flags:
                if flag == "--if-status" and row["status"] != value:
                    return self.done(command, 13)
                if flag == "--if-assignee" and (row.get("assignee") or "") != value:
                    return self.done(command, 13)
            for flag, value in flags:
                if flag == "--status":
                    row["status"] = value
                elif flag == "--assignee":
                    row["assignee"] = value
                elif flag == "--add-label":
                    row["labels"].append(value)
                elif flag == "--remove-label":
                    row["labels"].remove(value)
                elif flag == "--set-metadata":
                    key, _, val = value.partition("=")
                    row["metadata"][key] = json.loads(val) if val in ("true", "false") or val.lstrip("-").isdigit() else val
                elif flag == "--unset-metadata":
                    row["metadata"].pop(value, None)
                elif flag == "--append-notes":
                    row["notes"] = (row["notes"] + "\n" if row.get("notes") else "") + value
        self.touch(bead)
        return self.done(command)

    def writes(self):
        return [c for c in self.calls if c[1] not in ("list", "version", "schema")]

    def reads(self):
        return [c for c in self.calls if c[1] == "list"]


def workspace(root, database="testdb", project="p-1"):
    (root / ".beads").mkdir(exist_ok=True)
    (root / ".beads" / "metadata.json").write_text(json.dumps({"dolt_database": database, "project_id": project}))
    return root


@pytest.fixture(autouse=True)
def no_beads_dir(monkeypatch):
    monkeypatch.delenv("BEADS_DIR", raising=False)


@pytest.fixture
def world(tmp_path):
    bd = FakeBd()
    bd.add("b1", status="in_progress", assignee="dev")
    bd.add("b2")
    bd.add("s1")
    bd.edge("s1", "b1", "blocks")
    return bd, bc.BdCache(workspace(tmp_path), runner=bd, clock=lambda: 1792749600.0)


def by_id(bd, bead):
    return bc.validated(bd.rows[bead])


# -- identity and file ---------------------------------------------------------

def test_cache_file_is_named_for_the_dolt_database(world, tmp_path):
    bd, cache = world
    assert cache.path == tmp_path / ".atm-bd" / "testdb.bd-cache.sqlite3"
    assert bc.cache_path(tmp_path) == cache.path and cache.path.exists()
    assert cache.meta("dolt_database") == "testdb" and cache.meta("project_id") == "p-1"


def test_missing_metadata_is_typed(tmp_path):
    with pytest.raises(bc.CacheError) as error:
        bc.BdCache(tmp_path, runner=FakeBd())
    assert error.value.code == "BD_CACHE.NO_DATABASE"


def test_beads_dir_env_names_the_database(tmp_path, monkeypatch):
    workspace(tmp_path / "elsewhere", database="otherdb") if (tmp_path / "elsewhere").mkdir() is None else None
    monkeypatch.setenv("BEADS_DIR", str(tmp_path / "elsewhere" / ".beads"))
    assert bc.cache_path(tmp_path).name == "otherdb.bd-cache.sqlite3"


def test_file_from_another_project_is_refused(world, tmp_path):
    workspace(tmp_path, project="p-2")
    with pytest.raises(bc.CacheError) as error:
        bc.BdCache(tmp_path, runner=FakeBd())
    assert error.value.code == "BD_CACHE.WRONG_DATABASE"


def test_backend_check_does_not_need_bd_context(world):
    bd, cache = world
    report = cache.check_backend()
    assert not any(c[1] == "context" for c in bd.calls)
    assert report["bd_version"] == "1.3.1" and cache.meta("bd_version") == "1.3.1"
    assert cache.meta("bd_schema_version") == "1"


# -- bd schema pin and contract --------------------------------------------------

def test_pinned_model_matches_recorded_schema():
    assert hashlib.sha256(SCHEMA_TEXT.encode()).hexdigest() == bd_models.BD_SCHEMA_SHA256
    assert json.loads(SCHEMA_TEXT)["schema_version"] == bd_models.BD_SCHEMA_VERSION


def test_recorded_bd_rows_validate():
    rows = json.loads((FIXTURES / "list-rows.json").read_text())
    assert {r["id"] for r in rows} == {"ct-1", "ct-2", "ct-3"}
    for row in rows:
        out = bc.validated(row)
        assert "dependency_count" not in out and isinstance(out["dependencies"], list)
    ct2 = bc.validated(next(r for r in rows if r["id"] == "ct-2"))
    assert [bc.edge_key(d) for d in ct2["dependencies"]] == [("ct-2", "ct-1", "blocks")]


@pytest.mark.parametrize("bad", [{"surprise": 1}, {"status": "nope"}, {"created_at": "yesterday"}])
def test_contract_violation_is_typed_and_sync_stores_nothing(world, bad):
    bd, cache = world
    bd.rows["b2"].update(bad)
    with pytest.raises(bc.CacheError) as error:
        cache.sync("x")
    assert error.value.code == "BD_CACHE.SYNC_FAILED" and "BD_CACHE.BD_CONTRACT" in error.value.detail
    assert cache.list("x") == []
    assert cache.status()["phases"][0]["last_error"].startswith("BD_CACHE.BD_CONTRACT")


def test_schema_change_rebuilds_rows_and_keeps_the_journal(world):
    bd, cache = world
    cache.sync("x")
    cache.write("b2", "close", {"reason": "done"}, idem_key="k1", actor="san")
    assert cache.sync("x")["backend"]["rebuilt"] is False
    bd.schema = SCHEMA_TEXT.replace('"schema_version": 1', '"schema_version": 1 ')
    report = cache.sync("x")
    assert report["backend"]["rebuilt"] is True and report["backend"]["model_current"] is False
    assert report["listed"] == 3 and "--updated-after" not in bd.reads()[-1]
    assert cache.pending("b2") and cache.get("b2")["status"] == "closed"
    assert cache.drain()["applied"] == [1] and bd.rows["b2"]["status"] == "closed"


def test_model_current_when_live_schema_matches_pin(world):
    bd, cache = world
    assert cache.check_backend()["model_current"] is True


# -- reads -----------------------------------------------------------------------

def test_miss_fetches_once_then_serves_from_cache(world):
    bd, cache = world
    assert cache.get("b1")["status"] == "in_progress"
    assert cache.get("b1")["assignee"] == "dev"
    assert len(bd.reads()) == 1 and "--id" in bd.reads()[0]


def test_miss_on_unknown_bead_is_typed(world):
    bd, cache = world
    with pytest.raises(bc.CacheError) as error:
        cache.get("nope")
    assert error.value.code == "BD_CACHE.NOT_FOUND"


def test_write_is_visible_to_reads_but_cache_row_is_untouched(world):
    bd, cache = world
    cache.write("b1", "close", {"reason": "PASS"}, idem_key="k1", actor="san", precondition={"status": "in_progress"})
    assert cache.get("b1")["status"] == "closed"
    assert cache.cached("b1")["status"] == "in_progress"
    assert bd.writes() == []


def test_dependents_use_the_edge_index(world):
    bd, cache = world
    cache.sync("x")
    assert [b["id"] for b in cache.dependents("b1", "blocks")] == ["s1"]


# -- drain -----------------------------------------------------------------------

def test_drain_writes_in_order_reads_back_and_marks_applied(world):
    bd, cache = world
    cache.write("b1", "update", {"status": "open", "assignee": ""}, idem_key="k1", actor="san",
                precondition={"status": "in_progress", "assignee": "dev"})
    cache.write("b1", "update", {"add_labels": ["r2"], "append_notes": "reopened"}, idem_key="k2", actor="san",
                precondition={"status": "open"})
    report = cache.drain()
    assert report["applied"] == [1, 2] and not report["escalated"]
    assert bd.writes()[0][:3] == ["bd", "update", "b1"] and "--if-status" in bd.writes()[0]
    row = cache.cached("b1")
    assert (row["status"], row["assignee"], "r2" in row["labels"], row["notes"]) == ("open", "", True, "reopened")
    assert row == by_id(bd, "b1")
    assert cache.pending("b1") == []


def test_drain_pre_reads_all_pending_beads_in_one_call(world):
    bd, cache = world
    for seq, bead in enumerate(("b1", "b2", "s1", "b2")):
        cache.write(bead, "update", {"add_labels": [f"l{seq}"]}, idem_key=f"k{seq}", actor="san")
    bd.calls.clear()
    assert cache.drain()["applied"] == [1, 2, 3, 4]
    reads = bd.reads()
    assert reads[0][reads[0].index("--id") + 1] == "b1,b2,s1"
    assert len(reads) == 1 + 4  # one batched pre-read, then one read-back per write


def test_transient_write_failure_stays_pending_and_holds_later_entries_of_that_bead_only(world):
    bd, cache = world
    cache.write("b1", "close", {"reason": "PASS"}, idem_key="k1", actor="san")
    cache.write("b1", "update", {"add_labels": ["x"]}, idem_key="k2", actor="san")
    cache.write("b2", "close", {}, idem_key="k3", actor="san")
    bd.fail.append((lambda c: c[1] == "close" and c[2] == "b1", subprocess.TimeoutExpired("bd", 30)))
    report = cache.drain()
    assert report["retrying"] == [1] and report["applied"] == [3]
    assert cache.get("b1")["status"] == "closed"  # reads still see the pending write
    assert bd.rows["b1"]["status"] == "in_progress"
    assert cache.drain()["applied"] == [1, 2]
    assert bd.rows["b1"]["status"] == "closed" and "x" in bd.rows["b1"]["labels"]


def attempts(cache, seq):
    return cache.db.execute("SELECT attempts FROM journal WHERE seq=?", (seq,)).fetchone()[0]


@pytest.mark.parametrize("failure", [
    (lambda c: c[1] == "list" and "--id" in c, subprocess.TimeoutExpired("bd", 30)),  # pre-read timed out
    (lambda c: c[1] == "list" and "--id" in c, 1),  # pre-read exited nonzero (dolt connection refused)
    (lambda c: c[1] == "version", OSError("bd: not found")),  # backend check cannot run bd
], ids=["pre-read-timeout", "pre-read-exit", "backend-check"])
def test_unreachable_backend_is_an_outage_that_counts_no_attempt(world, failure):
    # A backend outage of more than MAX_ATTEMPTS drains used to escalate every pending write (bd-cache gap 2).
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    report = {}
    for _ in range(bc.MAX_ATTEMPTS + 2):
        bd.fail.append(failure)
        report = cache.drain()
        assert (report["ok"], report["deferred"], report["retrying"], report["escalated"]) == (False, [1], [], [])
        assert report["outage"]["code"] in bc.UNREACHABLE
    assert attempts(cache, 1) == 0 and bd.writes() == []
    outage = cache.status()["meta"]
    assert outage["outage_since"] and outage["outage_error"].startswith(report["outage"]["code"])
    assert cache.drain()["applied"] == [1]  # bd answers again: the write lands and the outage clears
    assert bd.rows["b2"]["status"] == "closed" and cache.outage() is None


def test_outage_keeps_its_start_time_and_latest_error(world):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    bd.fail.append((lambda c: c[1] == "version", OSError("first")))
    cache.drain()
    since = cache.outage()["since"]
    cache.clock = lambda: 1792749660.0
    bd.fail.append((lambda c: c[1] == "version", OSError("second")))
    cache.drain()
    assert cache.outage()["since"] == since and "second" in cache.outage()["error"]


def test_a_reachable_backend_refusing_the_write_still_counts_attempts(world):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    bd.fail.append((lambda c: c[1] == "close", 1))
    assert cache.drain()["retrying"] == [1] and attempts(cache, 1) == 1 and cache.outage() is None


def test_sync_records_an_outage_and_a_passing_sync_clears_it(world):
    bd, cache = world
    bd.fail.append((lambda c: c[1] == "list" and "-l" in c, subprocess.TimeoutExpired("bd", 30)))
    with pytest.raises(bc.CacheError) as raised:
        cache.sync("x")
    assert raised.value.code == "BD_CACHE.SYNC_FAILED" and raised.value.extra["cause"] == "BD_CACHE.BD_UNAVAILABLE"
    assert cache.outage() is not None
    cache.sync("x")
    assert cache.outage() is None


def test_retries_exhausted_escalate_and_never_silently_drop(world):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    for _ in range(bc.MAX_ATTEMPTS):
        bd.fail.append((lambda c: c[1] == "close", 1))
    for _ in range(bc.MAX_ATTEMPTS - 1):
        assert cache.drain()["retrying"] == [1]
    report = cache.drain()
    assert [e["seq"] for e in report["escalated"]] == [1]
    assert cache.status()["escalated"][0]["bead"] == "b2"
    assert cache.get("b2")["status"] == "open"  # escalated entries are not overlaid


def test_precondition_checked_at_write_time(world):
    bd, cache = world
    with pytest.raises(bc.CacheError) as error:
        cache.write("b1", "close", {}, idem_key="k1", actor="san", precondition={"status": "open"})
    assert error.value.code == "BD_CACHE.PRECONDITION"
    assert error.value.extra["unmet"] == {"status": {"expected": "open", "actual": "in_progress"}}


def test_concurrent_backend_change_escalates_instead_of_overwriting(world):
    bd, cache = world
    cache.write("b1", "update", {"status": "open"}, idem_key="k1", actor="san", precondition={"assignee": "dev"})
    bd.rows["b1"]["assignee"] = "someone-else"
    bd.touch("b1")
    report = cache.drain()
    assert report["escalated"][0]["reason"].startswith("precondition no longer holds")
    assert bd.writes() == [] and bd.rows["b1"]["status"] == "in_progress"
    assert cache.cached("b1")["assignee"] == "someone-else"


def test_bd_guard_refusal_exit_13_escalates(world):
    bd, cache = world
    cache.write("b1", "update", {"status": "open"}, idem_key="k1", actor="san", precondition={"status": "in_progress"})
    bd.fail.append((lambda c: c[1] == "update", 13))
    assert cache.drain()["escalated"][0]["reason"] == "bd refused: precondition changed (exit 13)"


def test_drain_on_absent_bead_escalates_not_found(world):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    del bd.rows["b2"]
    report = cache.drain()
    assert report["escalated"][0]["reason"] == "b2 not found in bd" and bd.writes() == []


def test_write_that_landed_before_a_crash_is_not_repeated(world):
    bd, cache = world
    cache.write("b1", "close", {"reason": "PASS"}, idem_key="k1", actor="san")
    bd(["bd", "close", "b1", "--reason", "PASS"])  # landed, but the journal mark was lost
    bd.calls.clear()
    assert cache.drain()["applied"] == [1]
    assert bd.writes() == []


def test_failed_read_back_marks_applied_and_stale_and_holds_the_bead_until_sync(world):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    cache.write("b2", "update", {"add_labels": ["after"]}, idem_key="k2", actor="san")
    reads = []
    bd.fail.append((lambda c: c[1] == "list" and c[c.index("--id") + 1] == "b2" and reads.append(1) is None
                    and len(reads) == 2, subprocess.TimeoutExpired("bd", 30)))
    report = cache.drain()  # drain read 1 = batch pre-read, read 2 = read-back of entry 1 (fails)
    assert report["applied"] == [1] and [e["seq"] for e in cache.pending("b2")] == [2]
    assert bd.writes() == [["bd", "close", "b2", "--actor", "san"]]
    assert cache.db.execute("SELECT stale FROM beads WHERE id='b2'").fetchone()[0] == 1
    assert cache.sync("x")["stale_refreshed"] == ["b2"]
    assert cache.cached("b2")["status"] == "closed"
    assert cache.drain()["applied"] == [2] and "after" in bd.rows["b2"]["labels"]


def test_duplicate_idem_key_is_one_entry_and_a_conflicting_reuse_is_refused(world):
    bd, cache = world
    first = cache.write("b2", "close", {}, idem_key="k1", actor="san")
    again = cache.write("b2", "close", {}, idem_key="k1", actor="san")
    assert (again["seq"], again["duplicate"]) == (first["seq"], True)
    with pytest.raises(bc.CacheError) as error:
        cache.write("b1", "close", {}, idem_key="k1", actor="san")
    assert error.value.code == "BD_CACHE.IDEM_CONFLICT"


def test_drain_is_single_writer(world, tmp_path):
    bd, cache = world
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    with cache.drain_lock() as held:
        assert held
        assert bc.BdCache(tmp_path, runner=bd).drain()["busy"] is True
    assert bd.writes() == []


# -- sync and prune --------------------------------------------------------------

def test_sync_is_incremental_and_never_replaces_a_newer_row(world):
    bd, cache = world
    first = cache.sync("x")
    assert first["listed"] == 3 and "--updated-after" not in bd.reads()[-1]
    assert [b["id"] for b in cache.list("x")] == ["b1", "b2", "s1"]
    bd.rows["b2"]["status"] = "blocked"
    bd.touch("b2")
    cache.sync("x")
    assert "--updated-after" in bd.reads()[-1]
    assert cache.cached("b2")["status"] == "blocked"
    older = dict(cache.cached("b2"), status="open", updated_at="2026-10-23T09:00:00Z")
    assert cache.store(older) is False
    assert cache.cached("b2")["status"] == "blocked"


def test_sync_failure_is_typed_and_recorded(world):
    bd, cache = world
    bd.fail.append((lambda c: c[1] == "list", subprocess.TimeoutExpired("bd", 30)))
    with pytest.raises(bc.CacheError) as error:
        cache.sync("x")
    assert error.value.code == "BD_CACHE.SYNC_FAILED"
    assert cache.status()["phases"][0]["last_error"]


def test_prune_refuses_with_unapplied_writes(world):
    bd, cache = world
    cache.sync("x")
    cache.write("b2", "close", {}, idem_key="k1", actor="san")
    with pytest.raises(bc.CacheError) as error:
        cache.prune("x")
    assert error.value.code == "BD_CACHE.PRUNE_BLOCKED"
    cache.drain()
    assert cache.prune("x")["pruned_beads"] == 3 and cache.list("x") == []


def test_newer_cache_schema_is_refused(tmp_path):
    workspace(tmp_path)
    bc.BdCache(tmp_path, runner=FakeBd()).db.execute("PRAGMA user_version=99")
    with pytest.raises(bc.CacheError) as error:
        bc.BdCache(tmp_path, runner=FakeBd())
    assert error.value.code == "BD_CACHE.SCHEMA_VERSION"


# -- request validation ----------------------------------------------------------

def test_bad_ops_are_typed(world):
    bd, cache = world
    for op, args in (("delete", {}), ("update", {}), ("dep_add", {"type": "blocks"}),
                     ("dep_add", {"depends_on": "b2", "type": "likes"}),
                     ("update", {"set_metadata": {"k": "5"}}), ("update", {"set_metadata": {"k": "true"}}),
                     ("update", {"set_metadata": {"k": " x"}}), ("update", {"set_metadata": {"k": 1.5}}),
                     ("update", {"set_metadata": {"k": None}})):
        with pytest.raises(bc.CacheError) as error:
            cache.write("b1", op, args, idem_key=f"{op}{args}", actor="san")
        assert error.value.code in ("BD_CACHE.BAD_OP", "BD_CACHE.BAD_ARGS")
    assert cache.db.execute("SELECT COUNT(*) FROM journal").fetchone()[0] == 0


def test_dep_add_overlays_then_lands_as_a_real_edge(world):
    bd, cache = world
    cache.write("b2", "dep_add", {"depends_on": "b1", "type": "blocks"}, idem_key="d1", actor="san")
    assert ("b2", "b1", "blocks") in {bc.edge_key(d) for d in cache.get("b2")["dependencies"]}
    cache.drain()
    assert [bc.edge_key(d) for d in cache.cached("b2")["dependencies"]] == [("b2", "b1", "blocks")]
    assert "pending" not in cache.cached("b2")["dependencies"][0]


def test_metadata_values_round_trip_through_bd_coercion(world):
    bd, cache = world
    cache.write("b2", "update", {"set_metadata": {"n": 5, "flag": True, "sha": "abc123", "raw": "[1]"}},
                idem_key="m1", actor="san")
    assert cache.get("b2")["metadata"] == {"n": 5, "flag": True, "sha": "abc123", "raw": "[1]"}
    cache.drain()
    command = next(c for c in bd.calls if c[1] == "update")
    assert {"n=5", "flag=true", "sha=abc123", "raw=[1]"} <= set(command)
    assert cache.cached("b2")["metadata"] == {"n": 5, "flag": True, "sha": "abc123", "raw": "[1]"}


def test_first_backend_check_records_schema_without_dropping_cached_rows(world):
    bd, cache = world
    cache.get("b1")
    assert cache.meta("bd_schema_sha256") is None
    report = cache.check_backend()
    assert report["rebuilt"] is False and cache.cached("b1") is not None
    assert cache.meta("bd_schema_sha256") == bd_models.BD_SCHEMA_SHA256


# -- escalation bookkeeping and bounded drain -----------------------------------

def escalate_one(bd, cache):
    cache.write("b1", "update", {"status": "open"}, idem_key="e1", actor="san", precondition={"assignee": "dev"})
    bd.rows["b1"]["assignee"] = "other"
    bd.touch("b1")
    assert cache.drain()["escalated"]


def test_escalations_are_unannounced_until_marked(world):
    bd, cache = world
    escalate_one(bd, cache)
    assert [e["seq"] for e in cache.unannounced_escalations()] == [1]
    assert cache.has_open_work()
    cache.mark_announced(1)
    assert cache.unannounced_escalations() == [] and not cache.has_open_work()
    assert cache.status()["escalated"][0]["seq"] == 1  # still listed until the lead resolves it


def test_resolve_closes_only_escalated_entries_and_unblocks_prune(world):
    bd, cache = world
    cache.sync("x")
    cache.write("b2", "close", {}, idem_key="p1", actor="san")
    with pytest.raises(bc.CacheError) as error:
        cache.resolve(1, "n/a", "lead")
    assert error.value.code == "BD_CACHE.NOT_ESCALATED"
    cache.drain()
    escalate_one(bd, cache)
    with pytest.raises(bc.CacheError):
        cache.prune("x")
    assert cache.resolve(2, "set by hand", "lead")["state"] == "resolved"
    assert cache.status()["escalated"] == []
    assert cache.get("b1")["status"] == "in_progress"  # resolved entries are never replayed
    assert cache.prune("x")["pruned_beads"] == 3


def test_pending_work_counts_as_open(world):
    bd, cache = world
    assert not cache.has_open_work()
    cache.write("b2", "close", {}, idem_key="o1", actor="san")
    assert cache.has_open_work()


def test_drain_deadline_defers_unstarted_entries_without_attempts(world):
    bd, cache = world
    cache.write("b1", "close", {}, idem_key="d1", actor="san")
    cache.write("b2", "close", {}, idem_key="d2", actor="san")
    ticks = iter([0.0, 10.0])
    report = cache.drain(deadline=5.0, monotonic=lambda: next(ticks))
    assert (report["applied"], report["deferred"]) == ([1], [2])
    row = cache.db.execute("SELECT state, attempts FROM journal WHERE seq=2").fetchone()
    assert (row["state"], row["attempts"]) == ("pending", 0)
    assert cache.drain()["applied"] == [2]


def test_sync_alert_round_trip(world):
    bd, cache = world
    assert cache.sync_alert("x") is None
    cache.set_sync_alert("x", "BD_CACHE.SYNC_FAILED: boom")
    assert cache.sync_alert("x") == "BD_CACHE.SYNC_FAILED: boom"
    cache.clear_sync_alert("x")
    assert cache.sync_alert("x") is None


def test_resolve_keeps_the_note_when_the_entry_had_no_error(world):
    bd, cache = world
    escalate_one(bd, cache)
    cache.db.execute("UPDATE journal SET last_error=NULL WHERE seq=1")
    cache.resolve(1, "set by hand", "lead")
    note = cache.db.execute("SELECT last_error FROM journal WHERE seq=1").fetchone()[0]
    assert note == " | resolved by lead: set by hand"
