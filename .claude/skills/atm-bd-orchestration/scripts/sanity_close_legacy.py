#!/usr/bin/env python3
# Frozen copy of the pre-bd-cache sanity close hook (2026-10-23), kept only for
# sanity-close-repair.py and repair tasks assigned before the cutover.
# Delete with sanity-close-repair.py, sanity-bead-repair.xml.j2 and
# tests/test_sanity_close_legacy.py once no `.bd-repair-` task remains open.
"""Sanity-only daemon task.close adapter. Existing close scripts remain independent.

Uses the selected result carried by --template/--vars, not a caller-owned file
path. ATM actions bypass hooks. bd failures transfer to a durable repair task
before the original task closes. Validation failures refuse without bead writes.
"""
# Maintenance and design tracking:
# Canonical issue: https://github.com/randlee/atm-bd-orchestration/issues/557
# Track this work in ONE GitHub issue for the integration package. Keep that
# issue's description accurate: remove erroneous or superseded requirements.
# Append comments for design decisions, requirements changes, implementation
# updates, and test results. Do not create separate issues for each gap or
# treat old comments as requirements when the current description corrects them.

from __future__ import annotations

import copy
from contextlib import contextmanager
import fcntl
import hashlib
from typing import Any
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import tomllib

TIMEOUT_MS = 2500
TEMPLATES = Path(__file__).resolve().parents[1] / "templates"
ASSIGNMENTS = {"assigned", "reassigned", "reopened"}


class Failure(Exception):
    def __init__(self, code, detail, owner="phase-lead", command=None):
        super().__init__(detail)
        self.code, self.owner, self.command = code, owner, command


def require(ok, field, detail, owner="sanity"):
    if not ok:
        raise Failure("SANITY_CLOSE.INVALID", f"{field}: {detail}", owner)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True).encode()).hexdigest()[:24]


class Journal:
    """Repair obligation and receipts, not a replacement for daemon observability."""
    def __init__(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        self.path = path
        self.db = sqlite3.connect(path, timeout=0.05)
        self.db.execute("CREATE TABLE IF NOT EXISTS records (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
        self.db.commit()

    def get(self, key) -> Any:  # a stored JSON record, or None
        row = self.db.execute("SELECT value FROM records WHERE key=?", (key,)).fetchone()
        return json.loads(row[0]) if row else None

    def put(self, key, value):
        with self.db:
            self.db.execute("INSERT INTO records VALUES (?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                            (key, json.dumps(value, sort_keys=True)))


@contextmanager
def task_lock(root, team, task):
    directory = root / ".atm-bd" / "sanity-close"
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / (digest([team, task]) + ".lock")).open("a") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise Failure("SANITY_CLOSE.BUSY", "another close or repair owns this task; retry after it completes", "sanity") from error
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


class Runtime:
    def __init__(self, envelope, runner=subprocess.run, clock=time.monotonic):
        self.envelope = envelope
        self.request = envelope["request"]
        self.root = Path.cwd()
        self.custom = envelope.get("custom", {})
        self.clock, self.runner = clock, runner
        self.started = clock()
        self.deadline = self.started + 1.8  # leave time for durable deferral and the JSON reply
        self.identity = envelope["caller"]["identity"]
        self.team = envelope["team"]
        self.task = self.request.get("task_id") or self.request.get("task_complete")
        self.env = {**os.environ, "ATM_IDENTITY": self.identity, "ATM_TEAM": self.team,
                    "BEADS_DIR": str(self.root / ".beads")}
        self.journal = None
        self.command_timeout: float | None = None  # per-command cap; the repair runner sets one
        self.plan: Any = None

    def run(self, command, *, timeout=None, retry=False):
        # Slow bd under host load is normal: a command may use the whole remaining
        # work deadline, and a read is retried once while budget remains.
        timeout = self.command_timeout if timeout is None else timeout
        attempts = 2 if retry else 1
        for attempt in range(attempts):
            remaining = self.deadline - self.clock()
            budget = remaining if timeout is None else min(timeout, remaining)
            if budget <= 0:
                raise Failure("SANITY_CLOSE.COMMAND", "hook work budget exhausted; retry in repair task", command=command)
            try:
                result = self.runner(command, cwd=self.root, env=self.env, text=True,
                                     capture_output=True, timeout=budget)
            except (OSError, subprocess.TimeoutExpired) as error:
                if attempt + 1 == attempts or self.deadline - self.clock() <= 0:
                    raise Failure("SANITY_CLOSE.COMMAND", str(error), command=command) from error
                continue
            if result.returncode:
                raise Failure("SANITY_CLOSE.COMMAND", (result.stderr or result.stdout).strip(), command=command)
            return result.stdout
        raise Failure("SANITY_CLOSE.COMMAND", "hook work budget exhausted; retry the close", command=command)

    def read_json(self, command):
        try:
            return json.loads(self.run(command, retry=True))
        except (ValueError, TypeError) as error:
            raise Failure("SANITY_CLOSE.DATA", f"invalid JSON from {command}: {error}", command=command) from error

    def bead(self, task):
        rows = self.read_json(["bd", "show", task, "--json"])
        require(isinstance(rows, list) and len(rows) == 1 and rows[0].get("id") == task,
                "bead", f"expected exactly {task}", "phase-lead")
        return rows[0]

    def events(self, task):
        data = self.read_json(["atm", "task", "events", task, "--json"])
        require(isinstance(data, dict) and isinstance(data.get("events"), list),
                "events", f"missing history for {task}", "phase-lead")
        return data["events"]

    def state(self):
        if self.journal is None:
            self.journal = Journal(self.root / ".atm-bd" / "sanity-close" / "journal.sqlite3")
        return self.journal

    def lead(self):
        try:
            config = tomllib.loads((self.root / ".atm-bd" / f"phase-{self.envelope['phase']}.toml").read_text())
            lead = config["lead"]
        except (OSError, ValueError, KeyError) as error:
            raise Failure("SANITY_CLOSE.CONFIG", f"phase lead unavailable: {error}") from error
        require(isinstance(lead, str) and bool(lead.strip()), "lead", "must be a nonempty identity", "phase-lead")
        return lead

    def action(self, command, task, identity, template, values, *, recipient=None):
        request = copy.deepcopy(self.request)
        for field in ("task_complete", "parent_message_id", "thread_mode", "placement",
                      "origin_message_id", "origin_timestamp", "acknowledges_message_id",
                      "activity_observation", "caller_chat_id"):
            request.pop(field, None)
        path = TEMPLATES / template
        request.update(caller_identity=identity, caller_team=self.team, task_id=task,
                       to=({"agent": recipient.split("@", 1)[0], "team": self.team} if recipient else None),
                       requires_ack=False, summary_override=None,
                       classification={"category": None, "tags": [], "content_format": None}, message_source={"Template": {
                           "canonical_template_path": str(path), "canonical_template_root": str(TEMPLATES),
                           "raw_file_bytes": list(path.read_bytes()), "input_defaults": {},
                           "var_file_values": values, "explicit_values": {}, "environment_values": {}}})
        if command == "task.close":
            request["task_op"] = {"op": "close", "outcome": "completed"}
        else:
            request.pop("task_op", None)
        return {"command": command, "as": identity, "request": request}

    def validation_error(self, error):
        error_id = digest([self.team, self.task, error.code, str(error)])
        try:
            escalation_to = self.lead()
        except Failure:
            escalation_to = "phase-lead"
        plan = self.plan or {}
        progress = {"completed_commands": plan.get("completed_commands", []),
                    "last_attempted_command": plan.get("last_attempted_command"),
                    "repair_task": plan.get("repair_task")}
        recovery = ("Keep the repair task open; phase lead must fix the reported operation or assignment conflict, "
                    "then rerun sanity-close-repair.py with its journal/key. Close repair only after repaired=true."
                    if plan.get("repair_task") else
                    "Phase lead: correct the reported data/operation, then retry the same native task close. "
                    "Inspect any attempted write before retrying; its outcome may be uncertain.")
        entry = {"task_id": self.task, "error_id": error_id, "code": error.code,
                 "escalation_to": escalation_to,
                 "escalation_task": f"{self.task}.close-error-{error_id}",
                 "detail": str(error), "owner": error.owner, "command": error.command,
                 "recovery": recovery, "state_summary": json.dumps(progress), "progress": progress}
        # One retained journal entry per cause; never claim that failed escalation was sent.
        try:
            state = self.state()
            previous = state.get("error:" + error_id) or {}
        except (sqlite3.Error, ValueError):
            # Escalation must not depend on a readable obligation journal.
            state = Journal(self.root / ".atm-bd" / "sanity-close" / "journal-escalations.sqlite3")
            previous = state.get("error:" + error_id) or {}
            entry["recovery"] = "Phase lead: restore the corrupt obligation journal from task history before retrying repair. " + recovery
        entry["notified"] = previous.get("notified", False)
        state.put("error:" + error_id, entry)
        if not entry["notified"] and not self.request.get("dry_run"):
            try:
                with tempfile.TemporaryDirectory(prefix="sanity-close-error-") as tmp:
                    path = Path(tmp) / "vars.json"
                    path.write_text(json.dumps({**entry, "original_task": self.task,
                                                "task_id": entry["escalation_task"]}))
                    # Reserve a bounded notification window after normal work expires.
                    self.deadline = max(self.deadline, min(self.clock() + 0.4, self.started + 2.3))
                    self.run(["atm", "task", "assign", self.lead(), "--task-id", entry["escalation_task"],
                              "--template", str(TEMPLATES / "hook-close-escalation.xml.j2"), "--vars", str(path)])
                entry["notified"] = True
            except Failure as escalation:
                entry["escalation_error"] = str(escalation)
            state.put("error:" + error_id, entry)
        print(json.dumps(entry), file=sys.stderr)
        return {"result": "refused", "message": json.dumps(entry)}


def selected_values(request):
    message_source = request.get("message_source")
    source = message_source.get("Template") if isinstance(message_source, dict) else None
    if not isinstance(source, dict):
        raise Failure("SANITY_CLOSE.INVALID", "message_source: use --template sanity-task-close.md.j2 --vars <selected-vars.json>", "sanity")
    for name in ("input_defaults", "var_file_values", "environment_values", "explicit_values"):
        require(isinstance(source.get(name, {}), dict), name, "must be a JSON object")
    values = {**source.get("input_defaults", {}), **source.get("environment_values", {}),
              **source.get("var_file_values", {}), **source.get("explicit_values", {})}
    return values


def prepare(runtime, sanity):
    """All validation/reads precede the first bead mutation."""
    values = selected_values(runtime.request)
    task = runtime.task
    require(values.get("task_id") == task, "task_id", "must match the closing sanity task")
    meta = sanity.get("metadata") or {}
    dev_id = meta.get("dev_bead")
    require(isinstance(dev_id, str) and dev_id, "metadata.dev_bead", "required on sanity bead", "phase-lead")
    require(values.get("checked_bead", dev_id) == dev_id, "checked_bead", "does not match sanity bead's dev_bead")
    values["checked_bead"] = dev_id
    require(any(d.get("id") == dev_id and d.get("dependency_type") == "blocks"
                for d in sanity.get("dependencies", [])), "dependencies", "sanity must be blocked by dev bead", "phase-lead")
    dev = runtime.bead(dev_id)
    labels = set(dev.get("labels", []))
    direct_finding_fix = "stage:finding" in labels and (dev.get("metadata") or {}).get("sanity_bead") == task
    require(bool({"stage:dev", "stage:fix"}.intersection(labels)) or direct_finding_fix,
            "checked_bead", "must be a dev/fix bead or a finding linked to this sanity bead", "phase-lead")
    # Commit is descriptive evidence, not a frozen implementation constraint.
    commit = values.get("commit") or (dev.get("metadata") or {}).get("commit") or "unspecified"
    values["commit"] = commit
    require(values.get("reviewer") == "sanity-selected", "reviewer", "must be sanity-selected")
    require(isinstance(values.get("run_id"), str) and values["run_id"], "run_id", "selected run ID required")
    verdict = values.get("verdict")
    if verdict == "CANNOT_RUN":
        raise Failure("SANITY_CLOSE.LEGACY_VERDICT",
                      "CANNOT_RUN is not an accepted verdict. Missing deliverables or missing/stale evidence are FAIL. "
                      "For an assignment conflict, unavailable required review service, or verification-tool failure, "
                      "escalate the exact conflicting records or failed operation and error to the phase lead. "
                      "Do not invent a verdict or run builds.", "phase-lead")
    require(verdict in ("PASS", "FAIL"), "verdict", "expected PASS or FAIL")
    require(verdict != "PASS" or dev.get("status") == "closed", "checked_bead.status",
            "PASS requires completed dev work; reopened prerequisites must be resolved by the developer", "phase-lead")
    findings = values.get("findings", [])
    require(isinstance(findings, list), "findings", "must be an array")
    for item in findings:
        require(isinstance(item, dict) and type(item.get("deliverable")) is int and item["deliverable"] >= 0
                and isinstance(item.get("issue"), str) and bool(item["issue"].strip()),
                "findings", "each item requires nonnegative deliverable (0=lint) and nonempty issue")
    require(verdict != "PASS" or not findings, "findings", "PASS cannot contain missing items")
    require(verdict != "FAIL" or bool(findings), "findings", "FAIL must identify missing items or lint failure")
    history = runtime.events(task)
    assignments = [event for event in history if event.get("event") in ASSIGNMENTS]
    require(bool(assignments), "assignment", "sanity task has no assignment", "phase-lead")
    assignment = assignments[-1]
    require(assignment.get("assignee") == runtime.identity, "caller", "only assigned sanity agent may close")
    dev_history = runtime.events(dev_id)
    dev_assignments = [event for event in dev_history if event.get("event") in ASSIGNMENTS]
    require(bool(dev_assignments), "dev assignment", "dev task has no assignment", "phase-lead")
    dev_agent = dev_assignments[-1].get("assignee")
    require(isinstance(dev_agent, str) and bool(dev_agent), "dev assignee", "missing dev task assignee", "phase-lead")
    require(dev.get("assignee") == dev_agent, "dev assignee", "bead and task assignees differ", "phase-lead")
    lead = runtime.lead()
    key = digest([runtime.team, task, assignment.get("message_id") or assignment.get("seq")])
    signature = digest([values.get("run_id"), verdict, findings])
    prior = runtime.state().get(key)
    require(prior is None or prior["signature"] == signature, "result", "conflicts with an already prepared close; lead must reconcile", "phase-lead")
    done = bool(dev_history and dev_history[-1].get("to_state") == "complete")
    commands = []
    if verdict == "PASS":
        if sanity.get("status") != "closed":
            commands.append(["bd", "close", task, "--reason", f"PASS at {commit}", "--actor", runtime.identity])
    else:
        if verdict == "FAIL" and dev.get("status") != "open":
            commands.append(["bd", "update", dev_id, "--status", "open", "--actor", runtime.identity])
        if sanity.get("status") != "open" or sanity.get("assignee"):
            commands.append(["bd", "update", task, "--status", "open", "--assignee", "", "--actor", runtime.identity])
    return {"key": key, "signature": signature, "task_id": task, "dev_id": dev_id,
            "dev_agent": dev_agent, "lead": lead, "values": values, "commands": commands,
            "dev_complete": done, "completed_commands": (prior or {}).get("completed_commands", []),
            "phase": runtime.envelope["phase"], "team": runtime.team}


def close_actions(runtime, plan):
    values = plan["values"]
    actions = []
    if values["verdict"] == "PASS" and not plan["dev_complete"]:
        actions.append(runtime.action("task.close", plan["dev_id"], plan["dev_agent"],
                                      "sanity-dev-close.md.j2", values))
    if values["verdict"] == "FAIL":
        actions.append(runtime.action("send", None, runtime.identity, "sanity-failed-template.md.j2", values,
                                      recipient=f"{plan['dev_agent']}@{runtime.team}"))
    original = copy.deepcopy(runtime.request)
    original["task_op"] = {"op": "close", "outcome": "completed"}
    actions.append({"command": "task.close", "request": original})
    return actions


def execute(runtime, plan):
    runtime.plan = plan
    state = runtime.state()
    state.put(plan["key"], plan)
    for command in plan["commands"]:
        if command in plan["completed_commands"]:
            continue
        plan["last_attempted_command"] = command
        state.put(plan["key"], plan)
        try:
            runtime.run(command)
            actual = runtime.bead(command[2])
            achieved = actual.get("status") == ("closed" if command[1] == "close" else "open")
            if "--assignee" in command:
                achieved = achieved and not actual.get("assignee")
            if not achieved:
                raise Failure("SANITY_CLOSE.WRITE_UNCONFIRMED", "bead did not reach requested state", command=command)
        except Failure as error:
            # Timeout means uncertain write: the repair re-reads the bead before retrying.
            plan["failure"] = {"command": command, "error": str(error)}
            repair_id = plan["task_id"] + ".bd-repair-" + plan["key"][:12]
            plan["repair_task"] = repair_id
            state.put(plan["key"], plan)
            repair_agent = runtime.custom.get("repair_agent") or runtime.identity
            repair_vars = {"task_id": repair_id, "original_task": plan["task_id"],
                           "journal": str(state.path), "key": plan["key"],
                           "phase": plan["phase"], "team": plan["team"], "phase_lead": plan["lead"],
                           "failure": plan["failure"], "root": str(runtime.root),
                           "repair_script": str(Path(__file__).with_name("sanity-close-repair.py").resolve())}
            actions = [runtime.action("task.assign", repair_id, plan["lead"],
                                      "sanity-bead-repair.xml.j2", repair_vars,
                                      recipient=f"{repair_agent}@{runtime.team}")]
            original = copy.deepcopy(runtime.request)
            original["task_op"] = {"op": "close", "outcome": "completed",
                                   "reason": f"Sanity {plan['values']['verdict']}; bead operations deferred to {repair_id}"}
            actions.append({"command": "task.close", "request": original})
            return {"result": "handled", "atm-action": actions}
        plan["completed_commands"].append(command)
        state.put(plan["key"], plan)
    plan["beads_applied"] = True
    state.put(plan["key"], plan)
    return {"result": "handled", "atm-action": close_actions(runtime, plan)}


def hook(envelope, runner=subprocess.run, clock=time.monotonic):
    if envelope.get("hook") != "task.close":
        return {"result": "not-applicable"}
    runtime = Runtime(envelope, runner, clock)
    # Task ids identify the phase; records identify the role. Repair tasks have no bead.
    prefix = runtime.custom.get("task_prefix", f"atm-{envelope['phase']}-")
    if not isinstance(runtime.task, str) or not runtime.task.startswith(prefix):
        return {"result": "not-applicable"}
    if ".close-error-" in runtime.task:
        return {"result": "pass-through"}
    if ".bd-repair-" in runtime.task:
        try:
            rows = runtime.state().db.execute("SELECT value FROM records").fetchall()
            plans = [json.loads(row[0]) for row in rows]
            matches = [p for p in plans if p.get("repair_task") == runtime.task]
            require(len(matches) == 1, "repair record", "expected one durable obligation", "phase-lead")
            runtime.plan = matches[0]
            require(runtime.plan.get("repaired"), "repair", "bead repair is unfinished; run the assigned repair command", "phase-lead")
            return {"result": "pass-through"}
        except Failure as error:
            return runtime.validation_error(error)
    # Explicit rollout routing keeps unrelated agents off bd entirely under load.
    members = runtime.custom.get("sanity_agents")
    if isinstance(members, list) and runtime.identity not in members:
        return {"result": "pass-through"}
    try:
        sanity = runtime.bead(runtime.task)
        if "stage:dev-sanity" not in sanity.get("labels", []):
            return {"result": "pass-through"}
        with task_lock(runtime.root, runtime.team, runtime.task):
            plan = prepare(runtime, sanity)
            if runtime.request.get("dry_run"):
                return {"result": "handled", "atm-action": close_actions(runtime, plan)}
            return execute(runtime, plan)
    except Exception as error:
        failure = error if isinstance(error, Failure) else Failure("SANITY_CLOSE.INTERNAL", f"{type(error).__name__}: {error}", "hook-maintainer")
        return runtime.validation_error(failure)


def doctor(envelope):
    directory = Path.cwd() / ".atm-bd" / "sanity-close"
    findings = []
    for path in sorted(directory.glob("journal*.sqlite3")):
        try:
            with sqlite3.connect(f"file:{path}?mode=ro", uri=True) as db:
                rows = list(db.execute("SELECT key,value FROM records"))
            for key, raw in rows:
                row = json.loads(raw)
                if key.startswith("error:") and not row.get("notified"):
                    findings.append({"severity": "warning", "code": "SANITY_CLOSE.ESCALATION_PENDING",
                                     "message": f"{row['task_id']}: {row['detail']}; owner={row['owner']}",
                                     "remediation": "Lead: inspect the journal and deliver the pending escalation."})
                elif row.get("repair_task") and not row.get("repaired"):
                    findings.append({"severity": "warning", "code": "SANITY_CLOSE.REPAIR_PENDING",
                                     "message": row["repair_task"], "remediation": "Run the assigned bead repair task."})
        except (sqlite3.Error, ValueError) as error:
            findings.append({"severity": "warning", "code": "SANITY_CLOSE.JOURNAL_UNREADABLE",
                             "message": f"{path}: {error}",
                             "remediation": "Phase lead: restore the journal from task history; inspect the fallback escalation journal."})
    return {"success": True, "message": "Sanity-only close hook; dev/fix/qa use default processing", "findings": findings}


if __name__ == "__main__":
    envelope = {}
    try:
        envelope = json.load(sys.stdin)
        reply = doctor(envelope) if sys.argv[1] == "doctor" else hook(envelope)
    except Exception as error:
        # A module/storage defect must never fall through to an unvalidated close.
        failure = Failure("SANITY_CLOSE.INTERNAL", f"{type(error).__name__}: {error}", "hook-maintainer")
        try:
            reply = Runtime(envelope).validation_error(failure)
        except Exception:
            print(f"SANITY_CLOSE.INTERNAL: {error}; escalation_to=phase-lead; could not persist/send escalation; caller must notify phase lead", file=sys.stderr)
            raise SystemExit(1)
    print(json.dumps(reply))
