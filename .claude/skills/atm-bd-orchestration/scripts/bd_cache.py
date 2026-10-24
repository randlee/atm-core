#!/usr/bin/env python3
"""Per-dolt-database bead cache and write journal (prototype; atm-bd-orchestration #557).

One SQLite file per dolt database, `<repo>/.atm-bd/<dolt_database>.bd-cache.sqlite3`,
holds the beads of every active phase as `bd` last reported them, plus a journal
of pending writes. Hooks and the console read the cache with pending journal
entries replayed on top and write only to the journal; all backend I/O goes
through `bd` in `drain` (writes) and `sync` (reads), so the cache works with any
backend `bd` supports. Backend rows are validated against models generated from
`bd schema` (bd_models.py); a changed `bd schema` rebuilds the cached rows and
keeps the journal.

Commands print one JSON object. Exit 0 = ok, 2 = usage, 3 = typed error (JSON
carries code, detail and the fix).
"""
import argparse
import contextlib
import copy
import datetime
import fcntl
import hashlib
import json
import os
import sqlite3
import subprocess
import sys
import time
from pathlib import Path
from typing import Optional

from pydantic import ValidationError

import bd_models

SCHEMA_VERSION = 3  # 3: journal.announced_at, state 'resolved' (v2 existed only in prototype scratch runs)
CACHE_DIR = Path(".atm-bd")
OPS = ("close", "update", "dep_add")
UPDATE_KEYS = {"status", "assignee", "add_labels", "remove_labels", "set_metadata", "unset_metadata", "append_notes"}
PRECONDITION_KEYS = {"status", "assignee"}
MAX_ATTEMPTS = 5
BD_TIMEOUT = 30.0
SYNC_SKEW = datetime.timedelta(minutes=2)
STATUSES = "open,in_progress,blocked,deferred,closed,pinned,hooked"
DERIVED_KEYS = ("dependency_count", "dependent_count")
# bd could not answer at all (cannot run, timed out, connection failed): a backend outage, never a write attempt.
UNREACHABLE = {"BD_CACHE.BD_UNAVAILABLE", "BD_CACHE.BD_VERSION_FAILED", "BD_CACHE.BD_SCHEMA_FAILED",
               "BD_CACHE.BD_LIST_FAILED"}

SCHEMA = """
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS beads (
  id TEXT PRIMARY KEY, phase TEXT, status TEXT, updated_at TEXT,
  row_json TEXT NOT NULL, synced_at TEXT NOT NULL, stale INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS beads_phase ON beads(phase, status);
CREATE TABLE IF NOT EXISTS edges (
  issue_id TEXT NOT NULL, depends_on_id TEXT NOT NULL, type TEXT NOT NULL,
  PRIMARY KEY (issue_id, depends_on_id, type));
CREATE INDEX IF NOT EXISTS edges_target ON edges(depends_on_id, type);
CREATE TABLE IF NOT EXISTS journal (
  seq INTEGER PRIMARY KEY AUTOINCREMENT, idem_key TEXT NOT NULL UNIQUE, phase TEXT,
  bead TEXT NOT NULL, op TEXT NOT NULL, args_json TEXT NOT NULL, precondition_json TEXT NOT NULL,
  actor TEXT NOT NULL, state TEXT NOT NULL CHECK (state IN ('pending','applied','escalated','resolved')),
  attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  announced_at TEXT);
CREATE INDEX IF NOT EXISTS journal_pending ON journal(state, bead, seq);
CREATE TABLE IF NOT EXISTS sync_state (
  phase TEXT PRIMARY KEY, last_started_at TEXT, last_ok_at TEXT, last_error TEXT);
"""


class CachedIssue(bd_models.Issue):
    """`bd list --json` row: the `bd schema` issue plus the fields --json adds but the schema omits."""

    comment_count: Optional[int] = None
    dependency_count: Optional[int] = None
    dependent_count: Optional[int] = None
    parent: Optional[str] = None


class CacheError(Exception):
    """Typed failure: code, detail and the exact fix for the calling agent."""

    def __init__(self, code, detail, fix, **extra):
        super().__init__(f"{code}: {detail}")
        self.code, self.detail, self.fix, self.extra = code, detail, fix, extra

    def as_json(self):
        return {"ok": False, "code": self.code, "detail": self.detail, "fix": self.fix, **self.extra}


def now_iso(clock=time.time):
    return datetime.datetime.fromtimestamp(clock(), datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def database_identity(root):
    """dolt database name and project id from bd's workspace metadata; no bd process."""
    beads_dir = Path(os.environ.get("BEADS_DIR") or Path(root) / ".beads")
    path = beads_dir / "metadata.json"
    try:
        data = json.loads(path.read_text())
        name, project = data["dolt_database"], data["project_id"]
    except (OSError, ValueError, KeyError) as error:
        raise CacheError("BD_CACHE.NO_DATABASE", f"cannot read dolt_database/project_id from {path}: {error}",
                         "run from the beads repository root (or set BEADS_DIR) after bd init")
    if not name or "/" in name:
        raise CacheError("BD_CACHE.NO_DATABASE", f"invalid dolt_database {name!r} in {path}", "repair .beads/metadata.json")
    return name, project


def cache_path(root):
    name, _ = database_identity(root)
    return Path(root) / CACHE_DIR / f"{name}.bd-cache.sqlite3"


def edge(issue_id, depends_on, kind):
    return {"issue_id": issue_id, "depends_on_id": depends_on, "type": kind}


def edge_key(dep):
    return dep["issue_id"], dep["depends_on_id"], dep["type"]


def validated(row):
    """Validate one `bd list --json` row against the generated model; return it without derived counts."""
    try:
        CachedIssue.model_validate(row)
    except ValidationError as error:
        first = error.errors()[0]
        raise CacheError("BD_CACHE.BD_CONTRACT",
                         f"{row.get('id', '?')}: {'.'.join(map(str, first['loc']))}: {first['msg']}",
                         f"bd output no longer matches bd_models.py (bd {bd_models.BD_VERSION}); regenerate it from "
                         "`bd schema` and update CachedIssue", bead=row.get("id"))
    row = copy.deepcopy(row)
    for key in DERIVED_KEYS:
        row.pop(key, None)
    row.setdefault("labels", [])
    row.setdefault("dependencies", [])
    if not isinstance(row.get("metadata"), dict):
        row["metadata"] = {}
    return row


def phase_of(row):
    for label in row.get("labels", []):
        if label.startswith("phase-"):
            return label[len("phase-"):]
    return None


def metadata_round_trips(value):
    """True when `bd --set-metadata key=<value>` stores exactly `value` (bd 1.3.1 coercion, probed live)."""
    if isinstance(value, bool) or isinstance(value, int):
        return True
    if not isinstance(value, str) or value != value.strip():
        return False
    try:
        parsed = json.loads(value)
    except ValueError:
        return True
    return isinstance(parsed, (str, list, dict))


def metadata_arg(value):
    return json.dumps(value) if isinstance(value, bool) else str(value)


def validate_entry(op, args, precondition):
    if op not in OPS:
        raise CacheError("BD_CACHE.BAD_OP", f"unknown op {op!r}", f"use one of {', '.join(OPS)}")
    if op == "close" and set(args) - {"reason"}:
        raise CacheError("BD_CACHE.BAD_ARGS", "close takes only reason", "pass {\"reason\": \"...\"}")
    if op == "update" and (not args or set(args) - UPDATE_KEYS):
        raise CacheError("BD_CACHE.BAD_ARGS", f"update keys must be a nonempty subset of {sorted(UPDATE_KEYS)}",
                         "remove unknown keys")
    for key, value in (args.get("set_metadata", {}) if op == "update" else {}).items():
        if not metadata_round_trips(value):
            raise CacheError("BD_CACHE.BAD_ARGS", f"set_metadata {key}={value!r} would not be stored as given: "
                             "bd coerces numeric, boolean and null-looking text and has no float or structured values",
                             "pass an int, a bool, or a string that is not numeric/boolean/null text")
    if op == "dep_add" and set(args) != {"depends_on", "type"}:
        raise CacheError("BD_CACHE.BAD_ARGS", "dep_add takes depends_on and type", "pass both keys")
    if op == "dep_add" and args["type"] not in {t.value for t in bd_models.Type}:
        raise CacheError("BD_CACHE.BAD_ARGS", f"unknown dependency type {args['type']!r}", "use a bd dependency type")
    if set(precondition) - PRECONDITION_KEYS:
        raise CacheError("BD_CACHE.BAD_PRECONDITION", f"precondition keys must be in {sorted(PRECONDITION_KEYS)}",
                         "remove unknown keys")


def apply_entry(row, op, args):
    """Pure overlay: the bead as it will be once this entry is written."""
    row = copy.deepcopy(row)
    if op == "close":
        row["status"] = "closed"
        if "reason" in args:
            row["close_reason"] = args["reason"]
    elif op == "update":
        if "status" in args:
            row["status"] = args["status"]
        if "assignee" in args:
            row["assignee"] = args["assignee"]
        labels = [label for label in row.get("labels", []) if label not in args.get("remove_labels", [])]
        labels += [label for label in args.get("add_labels", []) if label not in labels]
        row["labels"] = labels
        metadata = dict(row.get("metadata") or {})
        metadata.update(args.get("set_metadata", {}))
        for key in args.get("unset_metadata", []):
            metadata.pop(key, None)
        row["metadata"] = metadata
        if args.get("append_notes"):
            row["notes"] = (row["notes"] + "\n" if row.get("notes") else "") + args["append_notes"]
    elif op == "dep_add":
        new = edge(row["id"], args["depends_on"], args["type"])
        if edge_key(new) not in {edge_key(d) for d in row["dependencies"]}:
            row["dependencies"] = row["dependencies"] + [{**new, "pending": True}]
    return row


def achieved(row, op, args):
    """True when the backend row already shows this entry's effect (a write that landed before a crash)."""
    if op == "close":
        return row.get("status") == "closed"
    if op == "dep_add":
        return edge_key(edge(row["id"], args["depends_on"], args["type"])) in {edge_key(d) for d in row["dependencies"]}
    labels, metadata = set(row.get("labels", [])), row.get("metadata") or {}
    return all((
        "status" not in args or row.get("status") == args["status"],
        "assignee" not in args or (row.get("assignee") or "") == args["assignee"],
        set(args.get("add_labels", [])) <= labels,
        not set(args.get("remove_labels", [])) & labels,
        all(metadata.get(key) == value for key, value in args.get("set_metadata", {}).items()),
        not set(args.get("unset_metadata", [])) & set(metadata),
        not args.get("append_notes") or args["append_notes"] in (row.get("notes") or ""),
    ))


def unmet(row, precondition):
    return {key: {"expected": value, "actual": row.get(key) or ""}
            for key, value in precondition.items() if (row.get(key) or "") != value}


def command_for(bead, op, args, precondition, actor):
    if op == "close":
        command = ["bd", "close", bead]
        if "reason" in args:
            command += ["--reason", args["reason"]]
    elif op == "dep_add":
        command = ["bd", "dep", "add", bead, args["depends_on"], "--type", args["type"]]
    else:
        command = ["bd", "update", bead]
        if "status" in args:
            command += ["--status", args["status"]]
        if "assignee" in args:
            command += ["--assignee", args["assignee"]]
        for label in args.get("add_labels", []):
            command += ["--add-label", label]
        for label in args.get("remove_labels", []):
            command += ["--remove-label", label]
        for key, value in args.get("set_metadata", {}).items():
            command += ["--set-metadata", f"{key}={metadata_arg(value)}"]
        for key in args.get("unset_metadata", []):
            command += ["--unset-metadata", key]
        if args.get("append_notes"):
            command += ["--append-notes", args["append_notes"]]
        if "status" in precondition:
            command += ["--if-status", precondition["status"]]
        if "assignee" in precondition:
            command += ["--if-assignee", precondition["assignee"]]
    return command + ["--actor", actor]


class BdCache:
    def __init__(self, root, runner=subprocess.run, clock=time.time, db_path=None):
        self.root = Path(root)
        self.runner, self.clock = runner, clock
        self.database, self.project_id = database_identity(self.root)
        self.path = Path(db_path) if db_path else self.root / CACHE_DIR / f"{self.database}.bd-cache.sqlite3"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(self.path, timeout=5.0, isolation_level=None)
        self.db.row_factory = sqlite3.Row
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA busy_timeout=5000")
        version = self.db.execute("PRAGMA user_version").fetchone()[0]
        if version not in (0, SCHEMA_VERSION):
            raise CacheError("BD_CACHE.SCHEMA_VERSION", f"{self.path} is cache schema {version}, this code is {SCHEMA_VERSION}",
                             f"upgrade bd_cache, or move {self.path} aside to rebuild it from bd")
        self.db.executescript(SCHEMA)
        self.db.execute(f"PRAGMA user_version={SCHEMA_VERSION}")
        recorded = self.meta("project_id")
        if recorded is None:
            self.set_meta(dolt_database=self.database, project_id=self.project_id)
        elif recorded != self.project_id:
            raise CacheError("BD_CACHE.WRONG_DATABASE",
                             f"{self.path} belongs to project {recorded}, this workspace is {self.project_id}",
                             f"move {self.path} aside; it was built from a different dolt database")

    # -- meta ----------------------------------------------------------------
    def meta(self, key):
        found = self.db.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone()
        return found["value"] if found else None

    def set_meta(self, **values):
        self.db.executemany("INSERT OR REPLACE INTO meta VALUES (?,?)", [(k, str(v)) for k, v in values.items()])

    # -- backend (bd) --------------------------------------------------------
    def bd(self, command, timeout):
        try:
            return self.runner(command, cwd=self.root, text=True, capture_output=True, timeout=timeout)
        except (OSError, subprocess.TimeoutExpired) as error:
            raise CacheError("BD_CACHE.BD_UNAVAILABLE", f"{' '.join(command[:3])}: {error}",
                             "transient backend outage: the drain retries; a hook escalates or defers", command=command)

    def bd_json(self, command, timeout, code):
        result = self.bd(command, timeout)
        if result.returncode != 0:
            raise CacheError(code, (result.stderr or result.stdout).strip()[:500], "transient: retried by the next run",
                             command=command)
        try:
            return json.loads(result.stdout or "null")
        except ValueError as error:
            raise CacheError("BD_CACHE.BD_CONTRACT", f"{' '.join(command[:3])} printed invalid JSON: {error}",
                             "check the bd version against bd_models.py")

    def bd_rows(self, ids, timeout=BD_TIMEOUT):
        """Fresh validated rows for the given ids in one `bd list --id`; absent ids are missing from the result."""
        rows = self.bd_json(["bd", "list", "--id", ",".join(ids), "--status", STATUSES, "-n", "0", "--json"],
                            timeout, "BD_CACHE.BD_LIST_FAILED") or []
        self.clear_outage()  # only a data read proves bd answers; `bd version` can pass while lists time out
        return {row["id"]: row for row in map(validated, rows)}

    def check_backend(self, timeout=BD_TIMEOUT):
        """Confirm the bd schema; a changed schema rebuilds cached rows, keeping the journal.

        Identity comes from .beads/metadata.json (checked at open against the recorded project_id), not
        `bd context`, which refuses workspaces under temp directories that every other bd command accepts.
        """
        version = self.bd(["bd", "version"], timeout)
        if version.returncode != 0:
            raise CacheError("BD_CACHE.BD_VERSION_FAILED", (version.stderr or version.stdout).strip()[:500],
                             "check that bd is installed and on PATH")
        words = version.stdout.split()
        bd_version = words[2] if len(words) > 2 and words[:2] == ["bd", "version"] else version.stdout.strip()
        result = self.bd(["bd", "schema"], timeout)
        if result.returncode != 0:
            raise CacheError("BD_CACHE.BD_SCHEMA_FAILED", (result.stderr or result.stdout).strip()[:500],
                             "transient: retried by the next run")
        try:
            schema_version = json.loads(result.stdout).get("schema_version")
        except (ValueError, AttributeError) as error:
            raise CacheError("BD_CACHE.BD_CONTRACT", f"bd schema printed invalid JSON: {error}",
                             "check the bd version against bd_models.py")
        digest = hashlib.sha256(result.stdout.encode()).hexdigest()
        recorded = self.meta("bd_schema_sha256")
        rebuilt = recorded is not None and recorded != digest
        if recorded != digest:
            # First check records the schema (rows cached so far were validated by the pinned model);
            # a changed schema drops every cached row and sync cursor but never the journal.
            with self.transaction():
                if rebuilt:
                    self.db.execute("DELETE FROM edges")
                    self.db.execute("DELETE FROM beads")
                    self.db.execute("DELETE FROM sync_state")
                self.set_meta(bd_schema_sha256=digest, bd_version=bd_version, bd_schema_version=schema_version)
        return {"rebuilt": rebuilt, "model_current": digest == bd_models.BD_SCHEMA_SHA256, "bd_version": bd_version}

    # -- cache rows ----------------------------------------------------------
    def store(self, row, *, force=False):
        """Upsert a validated backend row; an older row never replaces a newer one unless forced (drain read-back)."""
        current = self.db.execute("SELECT updated_at FROM beads WHERE id=?", (row["id"],)).fetchone()
        if current and not force and (row.get("updated_at") or "") < (current["updated_at"] or ""):
            return False
        self.db.execute("INSERT OR REPLACE INTO beads(id, phase, status, updated_at, row_json, synced_at, stale) "
                        "VALUES (?,?,?,?,?,?,0)", (row["id"], phase_of(row), row.get("status"), row.get("updated_at"),
                                                   json.dumps(row, sort_keys=True), now_iso(self.clock)))
        self.db.execute("DELETE FROM edges WHERE issue_id=?", (row["id"],))
        self.db.executemany("INSERT OR IGNORE INTO edges VALUES (?,?,?)", [edge_key(d) for d in row["dependencies"]])
        return True

    def cached(self, bead):
        found = self.db.execute("SELECT row_json FROM beads WHERE id=?", (bead,)).fetchone()
        return json.loads(found["row_json"]) if found else None

    def entry(self, seq):
        """One journal entry by seq (state, attempts, last_error, ...)."""
        found = self.db.execute("SELECT * FROM journal WHERE seq=?", (seq,)).fetchone()
        if found is None:
            raise CacheError("BD_CACHE.NO_ENTRY", f"journal entry {seq} is absent", "check the seq")
        return dict(found)

    def pending(self, bead):
        return self.db.execute("SELECT * FROM journal WHERE bead=? AND state='pending' ORDER BY seq", (bead,)).fetchall()

    def view(self, row):
        for entry in self.pending(row["id"]):
            row = apply_entry(row, entry["op"], json.loads(entry["args_json"]))
        return row

    # -- public read/write ---------------------------------------------------
    def get(self, bead, *, miss_timeout=BD_TIMEOUT):
        row = self.cached(bead)
        if row is None:
            row = self.bd_rows([bead], miss_timeout).get(bead)
            if row is None:
                raise CacheError("BD_CACHE.NOT_FOUND", f"{bead} is not in the cache or in bd", "check the bead id")
            self.store(row)
        return self.view(row)

    def list(self, phase, status=None, label=None):
        rows = [self.view(json.loads(r["row_json"]))
                for r in self.db.execute("SELECT row_json FROM beads WHERE phase=? ORDER BY id", (phase,))]
        return [r for r in rows if (status is None or r.get("status") == status)
                and (label is None or label in r.get("labels", []))]

    def dependents(self, bead, edge_type=None):
        query = "SELECT issue_id FROM edges WHERE depends_on_id=?" + (" AND type=?" if edge_type else "")
        params = (bead, edge_type) if edge_type else (bead,)
        return [self.get(r["issue_id"]) for r in self.db.execute(query, params).fetchall()]

    def write(self, bead, op, args, *, idem_key, actor, precondition=None, miss_timeout=BD_TIMEOUT):
        """Journal one write. The precondition is checked now, against the view the caller acted on."""
        precondition = precondition or {}
        validate_entry(op, args, precondition)
        existing = self.db.execute("SELECT * FROM journal WHERE idem_key=?", (idem_key,)).fetchone()
        if existing:
            same = (existing["bead"], existing["op"], json.loads(existing["args_json"])) == (bead, op, args)
            if not same:
                raise CacheError("BD_CACHE.IDEM_CONFLICT", f"idem_key {idem_key} already journals a different write",
                                 "use a key derived from the whole operation", seq=existing["seq"])
            return {"ok": True, "seq": existing["seq"], "state": existing["state"], "duplicate": True}
        current = self.get(bead, miss_timeout=miss_timeout)
        missing = unmet(current, precondition)
        if missing:
            raise CacheError("BD_CACHE.PRECONDITION", f"{bead} no longer matches the state the write assumed",
                             "re-read the bead and decide again", unmet=missing)
        stamp = now_iso(self.clock)
        cursor = self.db.execute(
            "INSERT INTO journal(idem_key, phase, bead, op, args_json, precondition_json, actor, state, created_at, updated_at) "
            "VALUES (?,?,?,?,?,?,?,'pending',?,?)",
            (idem_key, phase_of(current), bead, op, json.dumps(args, sort_keys=True),
             json.dumps(precondition, sort_keys=True), actor, stamp, stamp))
        return {"ok": True, "seq": cursor.lastrowid, "state": "pending", "duplicate": False}

    # -- drain ---------------------------------------------------------------
    @contextlib.contextmanager
    def transaction(self):
        self.db.execute("BEGIN IMMEDIATE")
        try:
            yield
            self.db.execute("COMMIT")
        except BaseException:
            self.db.execute("ROLLBACK")
            raise

    @contextlib.contextmanager
    def drain_lock(self):
        with open(self.path.with_suffix(".drain.lock"), "a+") as handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                yield False
                return
            try:
                yield True
            finally:
                fcntl.flock(handle, fcntl.LOCK_UN)

    def _mark(self, seq, state, error=None, attempts_delta=0):
        self.db.execute("UPDATE journal SET state=?, last_error=?, attempts=attempts+?, updated_at=? WHERE seq=?",
                        (state, error, attempts_delta, now_iso(self.clock), seq))

    def _escalate(self, report, entry, reason, fresh):
        with self.transaction():
            if fresh is not None:
                self.store(fresh, force=True)
            self._mark(entry["seq"], "escalated", reason, 1)
        report["escalated"].append({"seq": entry["seq"], "bead": entry["bead"], "op": entry["op"],
                                    "actor": entry["actor"], "phase": entry["phase"], "reason": reason})

    def _retry(self, report, entry, error):
        if entry["attempts"] + 1 >= MAX_ATTEMPTS:
            self._escalate(report, entry, f"{error.code}: {error.detail} after {MAX_ATTEMPTS} attempts", None)
        else:
            self._mark(entry["seq"], "pending", f"{error.code}: {error.detail}", 1)
            report["retrying"].append(entry["seq"])

    def drain(self, deadline=None, monotonic=time.monotonic):
        """Apply pending entries in order; per bead, a failed entry holds the bead's later entries.

        With a deadline (a `monotonic()` value), entries not started by then stay pending, untouched, as `deferred`.
        """
        report = {"ok": True, "applied": [], "retrying": [], "escalated": [], "deferred": [], "busy": False}
        with self.drain_lock() as held:
            if not held:
                report["busy"] = True
                return report
            entries = self.db.execute("SELECT * FROM journal WHERE state='pending' ORDER BY seq").fetchall()
            if not entries:
                return report
            try:
                report["backend"] = self.check_backend()
                fresh = self.bd_rows(sorted({e["bead"] for e in entries}))
            except CacheError as error:
                if error.code not in UNREACHABLE:
                    if "backend" not in report:
                        raise
                    for entry in entries:
                        self._retry(report, entry, error)
                    return report
                # An outage: every entry stays pending with its attempts unchanged; the cron announces it once.
                self.record_outage(error)
                report.update(ok=False, outage=error.as_json(), deferred=[e["seq"] for e in entries])
                return report
            blocked = set()
            for entry in entries:
                seq, bead, op = entry["seq"], entry["bead"], entry["op"]
                if bead in blocked:
                    continue
                if deadline is not None and monotonic() >= deadline:
                    report["deferred"].append(seq)
                    continue
                args, precondition = json.loads(entry["args_json"]), json.loads(entry["precondition_json"])
                row = fresh.get(bead)
                if row is None:
                    self._escalate(report, entry, f"{bead} not found in bd", None)
                    blocked.add(bead)
                    continue
                if achieved(row, op, args):
                    with self.transaction():
                        self.store(row, force=True)
                        self._mark(seq, "applied")
                    report["applied"].append(seq)
                    continue
                missing = unmet(row, precondition)
                if missing:
                    self._escalate(report, entry, f"precondition no longer holds: {json.dumps(missing)}", row)
                    blocked.add(bead)
                    continue
                try:
                    result = self.bd(command_for(bead, op, args, precondition, entry["actor"]), BD_TIMEOUT)
                    if result.returncode not in (0, 13):
                        raise CacheError("BD_CACHE.BD_WRITE_FAILED", (result.stderr or result.stdout).strip()[:500],
                                         "transient: retried by the next drain")
                except CacheError as error:
                    blocked.add(bead)
                    self._retry(report, entry, error)
                    continue
                if result.returncode == 13:
                    self._escalate(report, entry, "bd refused: precondition changed (exit 13)", None)
                    blocked.add(bead)
                    continue
                try:
                    row = self.bd_rows([bead]).get(bead)
                except CacheError:
                    row = None
                with self.transaction():
                    if row is None:
                        self._mark(seq, "applied", "read-back failed; next sync re-reads the bead")
                        self.db.execute("UPDATE beads SET stale=1 WHERE id=?", (bead,))
                    else:
                        self.store(row, force=True)
                        self._mark(seq, "applied")
                if row is None:
                    blocked.add(bead)  # later entries for this bead wait for a fresh read
                else:
                    fresh[bead] = row
                report["applied"].append(seq)
        return report

    # -- sync ----------------------------------------------------------------
    def sync(self, phase, timeout=BD_TIMEOUT):
        try:
            backend = self.check_backend(timeout)
        except CacheError as error:
            if error.code in UNREACHABLE:
                self.record_outage(error)
            raise
        started = self.clock()
        state = self.db.execute("SELECT last_ok_at FROM sync_state WHERE phase=?", (phase,)).fetchone()
        command = ["bd", "list", "-l", f"phase-{phase}", "-n", "0", "--status", STATUSES, "--json"]
        if state and state["last_ok_at"]:
            since = datetime.datetime.strptime(state["last_ok_at"], "%Y-%m-%dT%H:%M:%SZ") - SYNC_SKEW
            command += ["--updated-after", since.strftime("%Y-%m-%dT%H:%M:%SZ")]
        self.db.execute("INSERT INTO sync_state(phase, last_started_at) VALUES (?,?) "
                        "ON CONFLICT(phase) DO UPDATE SET last_started_at=excluded.last_started_at",
                        (phase, now_iso(lambda: started)))
        try:
            listed = self.bd_json(command, timeout, "BD_CACHE.BD_LIST_FAILED") or []
            self.clear_outage()
            rows = [validated(row) for row in listed]
        except CacheError as error:
            self.db.execute("UPDATE sync_state SET last_error=? WHERE phase=?", (f"{error.code}: {error.detail}", phase))
            if error.code in UNREACHABLE:
                self.record_outage(error)
            raise CacheError("BD_CACHE.SYNC_FAILED", f"{error.code}: {error.detail}", error.fix, phase=phase,
                             cause=error.code)
        stale = [r["id"] for r in self.db.execute("SELECT id FROM beads WHERE phase=? AND stale=1", (phase,))]
        with self.transaction():
            stored = sum(self.store(row) for row in rows)
            self.db.execute("UPDATE sync_state SET last_ok_at=?, last_error=NULL WHERE phase=?",
                            (now_iso(lambda: started), phase))
        refreshed = []
        if stale:
            with contextlib.suppress(CacheError):
                for bead, row in self.bd_rows(stale, timeout).items():
                    with self.transaction():
                        self.store(row, force=True)
                    refreshed.append(bead)
        return {"ok": True, "phase": phase, "listed": len(rows), "stored": stored,
                "stale_refreshed": sorted(refreshed), "backend": backend}

    def prune(self, phase):
        open_entries = self.db.execute("SELECT COUNT(*) FROM journal WHERE phase=? AND state IN ('pending','escalated')",
                                       (phase,)).fetchone()[0]
        if open_entries:
            raise CacheError("BD_CACHE.PRUNE_BLOCKED", f"phase {phase} has {open_entries} pending or escalated writes",
                             "drain, or resolve the escalated entries with the lead, then prune")
        with self.transaction():
            ids = [r["id"] for r in self.db.execute("SELECT id FROM beads WHERE phase=?", (phase,))]
            self.db.executemany("DELETE FROM edges WHERE issue_id=?", [(i,) for i in ids])
            self.db.execute("DELETE FROM beads WHERE phase=?", (phase,))
            self.db.execute("DELETE FROM journal WHERE phase=?", (phase,))
            self.db.execute("DELETE FROM sync_state WHERE phase=?", (phase,))
        return {"ok": True, "phase": phase, "pruned_beads": len(ids)}

    # -- escalation bookkeeping (consumed by bd_cache_cron.py) ------------------
    def unannounced_escalations(self):
        return [dict(r) for r in self.db.execute(
            "SELECT * FROM journal WHERE state='escalated' AND announced_at IS NULL ORDER BY seq")]

    def mark_announced(self, seq):
        self.db.execute("UPDATE journal SET announced_at=? WHERE seq=?", (now_iso(self.clock), seq))

    def resolve(self, seq, note, actor):
        """The lead settled an escalated write by hand; the entry is closed and never replayed."""
        found = self.db.execute("SELECT state FROM journal WHERE seq=?", (seq,)).fetchone()
        if found is None or found["state"] != "escalated":
            raise CacheError("BD_CACHE.NOT_ESCALATED", f"journal entry {seq} is {found['state'] if found else 'absent'}",
                             "resolve only entries listed as escalated in status")
        self.db.execute("UPDATE journal SET state='resolved', last_error=COALESCE(last_error, '') || ?, updated_at=? WHERE seq=?",
                        (f" | resolved by {actor}: {note}", now_iso(self.clock), seq))
        return {"ok": True, "seq": seq, "state": "resolved"}

    def sync_alert(self, phase):
        return self.meta(f"sync_alert:{phase}")

    def set_sync_alert(self, phase, cause):
        self.set_meta(**{f"sync_alert:{phase}": cause})

    def clear_sync_alert(self, phase):
        self.db.execute("DELETE FROM meta WHERE key=?", (f"sync_alert:{phase}",))

    def outage(self):
        """{since, error, announced_at} while bd is unreachable, else None; cleared by the next `bd list` that answers."""
        since = self.meta("outage_since")
        return None if since is None else {"since": since, "error": self.meta("outage_error"),
                                           "announced_at": self.meta("outage_announced_at")}

    def record_outage(self, error):
        values = {"outage_error": f"{error.code}: {error.detail}"}
        if self.meta("outage_since") is None:
            values["outage_since"] = now_iso(self.clock)
        self.set_meta(**values)

    def mark_outage_announced(self):
        self.set_meta(outage_announced_at=now_iso(self.clock))

    def clear_outage(self):
        self.db.execute("DELETE FROM meta WHERE key IN ('outage_since', 'outage_error', 'outage_announced_at')")

    def has_open_work(self):
        return self.db.execute("SELECT COUNT(*) FROM journal WHERE state='pending' OR "
                               "(state='escalated' AND announced_at IS NULL)").fetchone()[0] > 0

    def status(self):
        counts = {f"{r['phase']}:{r['state']}": r["n"] for r in self.db.execute(
            "SELECT phase, state, COUNT(*) AS n FROM journal GROUP BY phase, state")}
        phases = [dict(r) | {"beads": self.db.execute("SELECT COUNT(*) FROM beads WHERE phase=?", (r["phase"],)).fetchone()[0]}
                  for r in self.db.execute("SELECT * FROM sync_state ORDER BY phase")]
        escalated = [dict(r) for r in self.db.execute(
            "SELECT seq, phase, bead, op, actor, last_error FROM journal WHERE state='escalated' ORDER BY seq")]
        meta = {r["key"]: r["value"] for r in self.db.execute("SELECT key, value FROM meta")}
        return {"ok": True, "db": str(self.path), "cache_schema_version": SCHEMA_VERSION, "meta": meta,
                "model_bd_version": bd_models.BD_VERSION, "journal": counts, "phases": phases, "escalated": escalated}


def main(argv=None):
    parser = argparse.ArgumentParser(description=(__doc__ or "bd-cache").splitlines()[0])
    parser.add_argument("--root", default=os.getcwd(), help="beads repository root (default: cwd)")
    parser.add_argument("--db", help="cache path (default: <root>/.atm-bd/<dolt_database>.bd-cache.sqlite3)")
    sub = parser.add_subparsers(dest="command", required=True)
    get = sub.add_parser("get"); get.add_argument("bead")
    lst = sub.add_parser("list"); lst.add_argument("--phase", required=True)
    lst.add_argument("--status"); lst.add_argument("--label")
    wr = sub.add_parser("write"); wr.add_argument("bead"); wr.add_argument("op", choices=OPS)
    wr.add_argument("--args", required=True, help="JSON object"); wr.add_argument("--precondition", default="{}")
    wr.add_argument("--idem-key", required=True); wr.add_argument("--actor", required=True)
    sub.add_parser("drain")
    sy = sub.add_parser("sync"); sy.add_argument("--phase", required=True, action="append")
    pr = sub.add_parser("prune"); pr.add_argument("--phase", required=True)
    sub.add_parser("status")
    rs = sub.add_parser("resolve"); rs.add_argument("seq", type=int)
    rs.add_argument("--note", required=True); rs.add_argument("--actor", required=True)
    args = parser.parse_args(argv)
    try:
        cache = BdCache(args.root, db_path=args.db)
        if args.command == "get":
            out = {"ok": True, "bead": cache.get(args.bead)}
        elif args.command == "list":
            out = {"ok": True, "beads": cache.list(args.phase, args.status, args.label)}
        elif args.command == "write":
            out = cache.write(args.bead, args.op, json.loads(args.args), precondition=json.loads(args.precondition),
                              idem_key=args.idem_key, actor=args.actor)
        elif args.command == "drain":
            out = cache.drain()
        elif args.command == "sync":
            out = {"ok": True, "phases": [cache.sync(phase) for phase in args.phase]}
        elif args.command == "prune":
            out = cache.prune(args.phase)
        elif args.command == "resolve":
            out = cache.resolve(args.seq, args.note, args.actor)
        else:
            out = cache.status()
    except CacheError as error:
        print(json.dumps(error.as_json(), sort_keys=True))
        return 3
    print(json.dumps(out, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
