#!/usr/bin/env python3
"""Resume a durable bead-repair obligation outside the daemon hook budget."""
# Maintenance and design tracking:
# Canonical issue: https://github.com/randlee/atm-bd-orchestration/issues/557
# Track this work in ONE GitHub issue for the integration package. Keep that
# issue's description accurate: remove erroneous or superseded requirements.
# Append comments for design decisions, requirements changes, implementation
# updates, and test results. Do not create separate issues for each gap or
# treat old comments as requirements when the current description corrects them.

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import tempfile

from sanity_close_legacy import Failure, Journal, Runtime, TEMPLATES, digest, require, task_lock


def repair(path, key, runner=None, context=None):
    # Invocation context survives loss/corruption of the obligation journal.
    plan = dict(context or {})
    state = None
    try:
        state = Journal(path)
        saved = state.get(key)
        require(saved is not None, "journal", "repair record missing; phase lead must restore the obligation from task history", "phase-lead")
        require(isinstance(saved, dict), "journal", "repair record malformed; phase lead must restore the obligation", "phase-lead")
        plan = saved
        with task_lock(path.resolve().parents[2], plan["team"], plan["task_id"]):
            return repair_locked(path, key, runner)
    except Exception as error:
        failure = error if isinstance(error, Failure) else Failure("SANITY_CLOSE.INTERNAL", f"{type(error).__name__}: {error}", "hook-maintainer")
        current = plan
        if state is not None:
            try:
                current = state.get(key) or plan
            except Exception:
                pass  # retain known invocation context and do not mask the original error
        if not isinstance(current, dict):
            current = plan
        routing = {**plan, **(context or {})}
        if not all(routing.get(field) for field in ("team", "phase", "task_id")):
            raise Failure("SANITY_CLOSE.REPAIR_CONTEXT", f"{failure}; repair invocation lacks phase/team/task context; phase lead must restore the repair assignment") from error
        envelope = {"team": routing["team"], "phase": routing["phase"],
                    "caller": {"identity": os.environ.get("ATM_IDENTITY", "")},
                    "request": {"task_id": plan.get("repair_task") or routing["task_id"] + ".bd-repair-" + key[:12]}}
        rt = Runtime(envelope, **({"runner": runner} if runner else {}))
        rt.root = path.resolve().parents[2]
        rt.env["BEADS_DIR"] = str(rt.root / ".beads")
        rt.deadline = rt.clock() + 30
        rt.command_timeout = 30
        rt.plan = {**current, "repair_task": envelope["request"]["task_id"]}
        lead = (context or {}).get("lead")
        if lead:
            # The assigned phase-lead identity remains available if phase config is lost too.
            rt.lead = lambda: lead
        rt.validation_error(failure)
        raise failure from error


def repair_locked(path, key, runner=None):
    state = Journal(path)
    plan = state.get(key)
    require(plan is not None, "journal", "repair record missing", "phase-lead")
    if plan.get("repaired"):
        return plan
    root = path.resolve().parents[2]
    original = plan["task_id"]
    envelope = {"team": plan["team"], "phase": plan["phase"],
                "caller": {"identity": os.environ.get("ATM_IDENTITY", "")},
                "request": {"task_id": original}}
    require(bool(envelope["caller"]["identity"]), "ATM_IDENTITY", "run as assigned repair agent")
    rt = Runtime(envelope, **({"runner": runner} if runner else {}))
    rt.root = root
    rt.env["BEADS_DIR"] = str(root / ".beads")
    rt.deadline = rt.clock() + 180
    rt.command_timeout = 30  # repair reads and writes run outside the daemon hook budget
    sanity = rt.bead(original)
    dev = rt.bead(plan["dev_id"])
    require((sanity.get("metadata") or {}).get("dev_bead") == plan["dev_id"]
            and any(d.get("id") == plan["dev_id"] and d.get("dependency_type") == "blocks"
                    for d in sanity.get("dependencies", [])),
            "relationship", "sanity/dev relationship changed; lead must reconcile", "phase-lead")
    dev_assignments = [e for e in rt.events(plan["dev_id"]) if e.get("event") in {"assigned", "reassigned", "reopened"}]
    require(bool(dev_assignments) and dev_assignments[-1].get("assignee") == plan["dev_agent"]
            and dev.get("assignee") == plan["dev_agent"],
            "dev assignment", "developer ownership changed; phase lead must reconcile stale repair", "phase-lead")
    assignments = [e for e in rt.events(original) if e.get("event") in {"assigned", "reassigned", "reopened"}]
    require(bool(assignments) and digest([rt.team, original, assignments[-1].get("message_id") or assignments[-1].get("seq")]) == key,
            "assignment", "sanity task was reassigned; lead must reconcile stale repair", "phase-lead")
    for command in plan["commands"]:
        bead = rt.bead(command[2])
        if command[1] == "close":
            achieved = bead.get("status") == "closed"
        else:
            achieved = bead.get("status") == "open" and ("--assignee" not in command or not bead.get("assignee"))
        if not achieved:
            plan["last_attempted_command"] = command
            state.put(key, plan)
            rt.run(command, timeout=30)
            after = rt.bead(command[2])
            require(after.get("status") == ("closed" if command[1] == "close" else "open")
                    and ("--assignee" not in command or not after.get("assignee")),
                    "bead state", "write was not confirmed; retry repair", "phase-lead")
        if command not in plan["completed_commands"]:
            plan["completed_commands"].append(command)
        state.put(key, plan)
    values = plan["values"]
    with tempfile.TemporaryDirectory(prefix="sanity-repair-") as tmp:
        variables = Path(tmp) / "vars.json"
        variables.write_text(json.dumps(values))
        if values["verdict"] == "PASS":
            events = rt.events(plan["dev_id"])
            if not events or events[-1].get("to_state") != "complete":
                command = ["atm", "task", "close", plan["dev_id"], "completed", "--as", plan["dev_agent"],
                           "--template", str(TEMPLATES / "sanity-dev-close.md.j2"), "--vars", str(variables)]
                plan["last_attempted_command"] = command
                state.put(key, plan)
                rt.run(command, timeout=30)
                after = rt.events(plan["dev_id"])
                if not after or after[-1].get("to_state") != "complete":
                    raise Failure("SANITY_CLOSE.WRITE_UNCONFIRMED",
                                  "dev task close not confirmed; retain repair and inspect daemon task history",
                                  command=["atm", "task", "events", plan["dev_id"], "--json"])
        elif values["verdict"] == "FAIL" and not plan.get("dev_notified"):
            command = ["atm", "send", f"{plan['dev_agent']}@{rt.team}", "--template",
                       str(TEMPLATES / "sanity-failed-template.md.j2"), "--vars", str(variables)]
            plan["last_attempted_command"] = command
            state.put(key, plan)
            rt.run(command, timeout=30)
            plan["dev_notified"] = True
            state.put(key, plan)
    plan["repaired"] = True
    state.put(key, plan)
    return plan


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal", type=Path, required=True)
    parser.add_argument("--key", required=True)
    parser.add_argument("--phase", required=True)
    parser.add_argument("--team", required=True)
    parser.add_argument("--original-task", required=True)
    parser.add_argument("--lead", required=True)
    args = parser.parse_args()
    try:
        result = repair(args.journal, args.key, context={"phase": args.phase, "team": args.team,
                                                       "task_id": args.original_task, "lead": args.lead})
        print(json.dumps({"ok": True, "task_id": result["task_id"], "repaired": True}))
    except Failure as error:
        print(json.dumps({"error": error.code, "detail": str(error), "command": error.command,
                          "owner": error.owner, "recovery": "Keep repair task open; fix cause and retry."}))
        raise SystemExit(1)
