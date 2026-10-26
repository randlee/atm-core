#!/usr/bin/env python3
"""Close orchestration beads, hand off work, and close the dev task at dev complete.

Every bd, git and gh command is bounded by `ATM_BD_GATE_TIMEOUT_S` seconds (default 180, shared with
assignment-gates.py); a timeout is `COMMAND.FAILED` whose fix says to retry once or raise the variable.
Bead writes go through the bd-cache journal and are drained at once (BD-CACHE.md, Scope); a write not yet
applied is `BEADS.WRITE_PENDING` and a rerun with the same vars applies it once.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import bd_cache
import repo_config
import sc_render
from command_timeout import TIMEOUT_ENV, command_timeout, import_timeout
from stack_top import select_stack_top

TEMPLATES = Path(__file__).resolve().parents[1] / "templates"
ROLES = ("dev", "fix", "sanity", "qa")
COMMAND_TIMEOUT = import_timeout()
ASSIGNMENT_EVENTS = frozenset(("assigned", "reassigned", "reopened"))
TERMINAL_STATES = frozenset(("complete",))
COMPLETED = "completed"
REFUSED = "refused"
# Dolt drops connections under host load (load 40-55 with parallel cargo builds): a dropped bd call is retried.
# Worst case for one bd call that keeps dropping = (DOLT_RETRIES + 1) * COMMAND_TIMEOUT + DOLT_RETRIES * DOLT_RETRY_SLEEP_S;
# a call that times out is not retried and fails after COMMAND_TIMEOUT. See docs/requirements/task-close-hook.md.
DOLT_DROP = re.compile(r"invalid connection|i/o timeout", re.IGNORECASE)
DOLT_RETRIES = 3
DOLT_RETRY_SLEEP_S = 10
# Bead writes go through the bd-cache journal and are drained at once; a drain held by the cron is waited for.
DRAIN_TRIES = 5
DRAIN_BUSY_SLEEP_S = 2


def is_terminal(events):
    return bool(events) and events[-1].get("to_state") in TERMINAL_STATES


def load_sample():
    """Portable host load: os.getloadavg plus load1 as a percentage of CPU count (no new dependencies)."""
    try:
        one, five, fifteen = os.getloadavg()
    except (AttributeError, OSError):
        return {"load_avg": None, "cpu_pct": None}
    return {"load_avg": [round(one, 2), round(five, 2), round(fifteen, 2)],
            "cpu_pct": round(100 * one / (os.cpu_count() or 1), 1)}


def bd_value(item):
    """A metadata value as bd stores it: lists and objects as their JSON text (readers decode them, e.g.
    sanity_round_key) and integer text as the integer (a vars file carries pr_number as "351")."""
    if isinstance(item, (list, dict)):
        return json.dumps(item, separators=(",", ":"))
    if isinstance(item, str) and item.isdigit() and str(int(item)) == item:
        return int(item)
    return item


class CloseError(RuntimeError):
    def __init__(self, code, message, fix):
        super().__init__(message)
        self.code, self.fix = code, fix


class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise CloseError("ARGS.INVALID", message, "Supply --role dev|fix|sanity|qa and --vars <shared-vars.json>.")


def header(path):
    return yaml.safe_load(path.read_text(encoding="utf-8").split("---", 2)[1])


def read_vars(path):
    try:
        text = path.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as error:
        raise CloseError("VARS.INVALID", f"Cannot read vars file {path}: {error}",
                         f"Provide a readable UTF-8 JSON file at --vars {path}.") from error
    try:
        values = json.loads(text)
    except json.JSONDecodeError as error:
        raise CloseError("VARS.INVALID", f"{path}:{error.lineno}:{error.colno}: {error.msg}",
                         f"Correct JSON at line {error.lineno}, column {error.colno} in {path}.") from error
    if not isinstance(values, dict):
        raise CloseError("VARS.INVALID", f"{path}: expected a JSON object, got {type(values).__name__}",
                         f"Replace the top-level value in {path} with the lead's shared vars object.")
    return values


def merge_result(values, result, role):
    """Join the fixer's result file to the lead's dispatch vars; each field lives in exactly one of them."""
    fields = set(header(TEMPLATES / "task-close-vars.j2")["result_variables_by_role"][role])
    unknown = result.keys() - fields
    if unknown:
        raise CloseError("VARS.UNKNOWN", ", ".join(sorted(unknown)), f"Remove fields that are not {role} result fields from --result.")
    overlap = values.keys() & result.keys()
    if overlap:
        raise CloseError("VARS.INVALID", ", ".join(sorted(overlap)),
                         "Pass each completion field once: remove it from the lead's dispatch file or from --result.")
    return {**values, **result}


def validate_shared_vars(values, role):
    """Strictly render the shared dispatch contract before inspecting live state."""
    validate(values, role)
    contract = header(TEMPLATES / "task-close-vars.j2")
    fields = contract["render_variables_by_role"][role]
    if role == "fix" and values["outcome"] == "not_reproducible":
        fields = [name for name in fields if name not in ("branch", "stack_bead")]
    if role == "sanity" and values["outcome"] == "not_reproducible":
        fields = [name for name in fields if name != "stack_bead"]
    render_values = {name: values[name] for name in fields if name in values}
    contract.update(required_variables=fields, defaults={})
    try:
        with tempfile.TemporaryDirectory(prefix="atm-task-close-contract-") as directory:
            template = Path(directory) / "task-close-vars.j2"
            template.write_text("---\n" + yaml.safe_dump(contract) + "---\n{{ '' }}\n", encoding="utf-8")
            sc_render.render(template, render_values)
    except sc_render.RenderError as error:
        raise CloseError("VARS.INVALID", str(error),
                         "Supply every field required by the shared task-close vars contract.") from error


def read_toml_setting(path, fields):
    name = ".".join(fields)
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, tomllib.TOMLDecodeError) as error:
        raise CloseError("CONFIG.INVALID", f"Cannot read {name} from {path}: {error}",
                         f"Create or repair UTF-8 TOML file {path} and set {name} to a non-empty string.") from error
    for field in fields:
        if not isinstance(data, dict) or field not in data:
            raise CloseError("CONFIG.INVALID", f"{path}: missing {name}",
                             f"Set {name} to a non-empty string in {path}.")
        data = data[field]
    if not isinstance(data, str) or not data.strip():
        raise CloseError("CONFIG.INVALID", f"{path}: {name} must be a non-empty string",
                         f"Set {name} to a non-empty string in {path}.")
    return data


def read_config(root, role, verdict):
    path = root / repo_config.RELATIVE_PATH
    try:
        config = repo_config.load(root)
    except (repo_config.ConfigError, OSError, UnicodeError) as error:
        if isinstance(error, repo_config.ConfigNotFound):
            fix = f"Run the atm-bd-orchestration installer for {root} to create {path}."
        elif "max_sanity_rounds" in str(error):
            fix = f"Set max_sanity_rounds to a positive integer in the consumer registry and rerun the installer to regenerate {path}."
        elif "lead belongs" in str(error):
            fix = f"Move lead from {path} into .atm-bd/phase-<x>.toml and rerun the installer to remove the repository lead."
        else:
            fix = f"Repair the readable UTF-8 YAML mapping in {path} at the location reported in the error."
        raise CloseError("CONFIG.INVALID", f"{path}: {error}",
                         fix) from error
    fields = ["dev_sanity_member"] if role in ("dev", "fix") else ["qa_member"] if role == "sanity" and verdict == "PASS" else ["team_lead_member"] if role == "sanity" and verdict == "FAIL" else []
    for field in fields:
        value = config.get(field)
        if not isinstance(value, str) or not value.strip():
            raise CloseError("CONFIG.INVALID", f"{path}: {field} must be a non-empty string",
                             f"Set {field} in the consumer registry and rerun the installer to regenerate {path}.")
    return config


def sanity_round(metadata, bead):
    count = metadata.get("sanity_round", 0)
    if isinstance(count, bool) or not isinstance(count, int) or count < 0:
        raise CloseError("BEAD.INVALID", f"{bead}: metadata.sanity_round must be a non-negative integer, got {count!r}",
                         f"Correct metadata.sanity_round on bead {bead} to the recorded non-negative FAIL count with bd update {bead} --metadata, preserving other metadata.")
    return count


def sanity_round_key(metadata, bead):
    key = metadata.get("sanity_round_key")
    if key is None:
        return None
    if isinstance(key, str):
        try:
            key = json.loads(key)
        except ValueError:
            key = None
    if (not isinstance(key, list) or len(key) != 4 or not isinstance(key[0], str)
            or isinstance(key[1], bool) or not isinstance(key[1], (str, int))
            or not isinstance(key[2], str) or not isinstance(key[3], str)):
        raise CloseError("BEAD.INVALID", f"{bead}: invalid metadata.sanity_round_key",
                         f"Repair metadata.sanity_round_key on bead {bead}.")
    return key


def assignment_details(events, task):
    for index in range(len(events) - 1, -1, -1):
        assignment = events[index]
        if assignment.get("event") in ASSIGNMENT_EVENTS:
            assignment_id = assignment.get("message_id") or assignment["seq"]
            round_number = 1 + sum(event.get("event") == COMPLETED for event in events[:index])
            return assignment, assignment_id, round_number
    raise CloseError("REPLAY.CONFLICT", f"{task} has no assignment event", "Assign the tracked task before closing it.")


def validate_command_json(argv, data):
    producer = " ".join(argv)

    def require(condition, field, expected):
        if not condition:
            raise CloseError("COMMAND.INVALID", f"{producer}: {field} must be {expected}",
                             f"Repair {field} in the JSON returned by `{producer}` to be {expected}, then retry.")

    if argv[:2] in (["bd", "show"], ["bd", "list"]):
        require(isinstance(data, list), "$", "an array of bead objects")
        for index, row in enumerate(data):
            field = f"$[{index}]"
            require(isinstance(row, dict), field, "a bead object")
            metadata = row.get("metadata")
            require(metadata is None or isinstance(metadata, dict), field + ".metadata", "an object or null")
    elif argv[:3] == ["atm", "task", "events"]:
        require(isinstance(data, dict), "$", "an object")
        events = data.get("events")
        require(isinstance(events, list), "$.events", "an array")
        for index, event in enumerate(events):
            field = f"$.events[{index}]"
            require(isinstance(event, dict), field, "an event object")
            if event.get("event") in ASSIGNMENT_EVENTS:
                require(isinstance(event.get("at"), str) and bool(event["at"]), field + ".at", "a timestamp string")
                require(bool(event.get("message_id")) or isinstance(event.get("seq"), int), field + ".message_id/seq", "an assignment event identifier")
    elif argv[:2] == ["gh", "api"]:
        require(isinstance(data, list), "$", "an array of PR objects")
        for index, pr in enumerate(data):
            field = f"$[{index}]"
            require(isinstance(pr, dict), field, "a PR object")
            require("number" in pr, field + ".number", "a PR number")
            require(isinstance(pr.get("head"), dict), field + ".head", "an object")
            require(isinstance(pr["head"].get("sha"), str), field + ".head.sha", "a string")
    return data


def validate(values, role):
    if role == "dev" and values.get("outcome") == "not_reproducible":
        raise CloseError("VARS.INVALID", "not_reproducible is a fix outcome", "Use the fix role for an unreproducible finding.")
    contract = header(TEMPLATES / "task-close-vars.j2")
    role_required = contract.get("required_variables_by_role")
    if not isinstance(role_required, dict) or role not in role_required:
        raise CloseError("VARS.INVALID", "task-close-vars must declare requirements by role", "Repair the role-specific required_variables mapping.")
    required = set(role_required[role]) | set(contract.get("result_variables_by_role", {}).get(role, []))
    declared = set(contract["required_variables"])
    missing = required - values.keys()
    if role == "fix" and values.get("outcome") == "not_reproducible":
        missing -= {"branch", "commit", "pr_number", "stack_bead"}
    if role == "sanity" and values.get("outcome") == "not_reproducible":
        missing -= {"stack_bead"}
    unknown = values.keys() - declared - contract.get("defaults", {}).keys()
    if missing:
        raise CloseError("VARS.MISSING", ", ".join(sorted(missing)), "Supply every shared template variable in the lead's vars file.")
    if unknown:
        raise CloseError("VARS.UNKNOWN", ", ".join(sorted(unknown)), "Remove variables no shared template declares.")
    names = ["phase", "sprint", "primary_checkout", "worktree_path", "checked_bead", "dev_task_id", "sanity_task_id", "qa_task_id"]
    stack_required = values["outcome"] != "not_reproducible"
    if role in ("dev", "fix", "sanity") and stack_required:
        names.append("stack_bead")
    if role == "fix" and values["outcome"] == "not_reproducible" and not values.get("stack_bead"):
        names.append("pr_target")
    if values["outcome"] != "not_reproducible":
        names += ["branch"] if role in ("dev", "fix") else ["branch", "commit", "pr_number"]
    elif not re.search(r"\S+:\d+", str(values["evidence"])):
        raise CloseError("VARS.INVALID", "not_reproducible requires evidence file:line", "Supply the authoritative absence location.")
    for name in names:
        if not str(values[name] or "").strip():
            raise CloseError("VARS.EMPTY", name, f"Populate {name} in the shared vars file.")
    for name in ("primary_checkout", "worktree_path", "phase", *(["stack_bead"] if role in ("dev", "fix", "sanity") and stack_required else [])):
        if not isinstance(values[name], str):
            raise CloseError("VARS.INVALID", f"{name} must be a string, got {type(values[name]).__name__}",
                             f"Set {name} to a non-empty string in the lead's shared vars file.")
    if role in ("sanity", "qa") and values["verdict"] not in ("PASS", "FAIL", "CANNOT_RUN"):
        raise CloseError("VARS.INVALID", "verdict must be PASS, FAIL or CANNOT_RUN", "Supply the selected review verdict.")
    if role == "sanity" and (isinstance(values["round"], bool) or not isinstance(values["round"], int) or values["round"] < 1):
        raise CloseError("VARS.INVALID", "round must be a positive integer", "Supply the current sanity assignment round.")
    if role == "qa" and values["verdict"] == "CANNOT_RUN":
        raise CloseError("VARS.INVALID", "QA CANNOT_RUN is not a completed review", "Use the QA refusal workflow.")
    undone = values.get("undone", []) if role in ("dev", "fix") else values["undone"]
    if not isinstance(undone, list) or any(
        not isinstance(item, dict) or isinstance(item.get("number"), bool)
        or not isinstance(item.get("number"), int) or item["number"] < 1
        or not isinstance(item.get("reason"), str) or not item["reason"].strip()
        for item in undone
    ):
        raise CloseError("VARS.INVALID", "undone must contain numbered deliverables and verbatim reasons", "Supply the selected undone deliverables.")
    if role == "sanity" and values["verdict"] == "FAIL" and not values["undone"]:
        raise CloseError("VARS.INVALID", "FAIL has no undone deliverables", "Supply the selected undone deliverables.")
    if values["dev_task_id"] != values["checked_bead"] or values["sanity_task_id"] != values["sanity_bead"] or values["qa_task_id"] != values["qa_bead"]:
        raise CloseError("VARS.INVALID", "task ids and bead ids differ", "Use bead ids as the corresponding task ids.")


class Hook:
    def __init__(self, role, values, path, runner, clock, sleep=time.sleep, sample=load_sample):
        self.role, self.values, self.path = role, values, Path(path).resolve()
        self.runner, self.clock, self.sleep, self.sample = runner, clock, sleep, sample
        self.root = Path(values["primary_checkout"]).resolve()
        self.worktree = Path(values["worktree_path"]).resolve()
        self.identity = os.environ.get("ATM_IDENTITY", "")
        if not self.identity:
            raise CloseError("VARS.INVALID", "ATM_IDENTITY is not set", "Run as the assigned ATM member.")
        config = read_config(self.root, role, values.get("verdict", ""))
        self.config = config
        self.lead = read_toml_setting(self.root / ".atm-bd" / f"phase-{values['phase']}.toml", ("lead",))
        team = read_toml_setting(self.root / ".atm.toml", ("atm", "default_team"))
        self.env = dict(os.environ)
        self.lead_env = {**self.env, "ATM_IDENTITY": self.lead, "ATM_TEAM": team}
        self.github = "ok"
        self.github_error = ""
        self.closed, self.assigned = [], []
        self.task = values[{"dev": "dev_task_id", "fix": "dev_task_id", "sanity": "sanity_task_id", "qa": "qa_task_id"}[role]]
        self.key = [self.task, values.get("verdict", ""), values.get("commit", "")]
        self.records, self.task_history = {}, {}
        self.attempts = []
        self.beads = None
        self.assignment_id = None

    def run(self, argv, *, lead=False, git=False, check=True):
        started = self.clock()
        result = None
        for attempt in range(DOLT_RETRIES + 1):
            began = self.clock()
            result, error = None, None
            try:
                result = self._run_once(argv, lead=lead, git=git)
            except CloseError as failure:
                error = failure
            retry = (error is None and result is not None and argv[0] == "bd" and bool(result.returncode) and attempt < DOLT_RETRIES
                     and bool(DOLT_DROP.search((result.stderr or "") + (result.stdout or ""))))
            outcome = ("timeout" if error is not None and "timed out" in str(error) else "error" if error is not None
                       else "dolt_drop_retry" if retry else "failed" if result is not None and result.returncode else "ok")
            if outcome != "ok" or attempt:
                self._log_attempt(argv, attempt + 1, began, outcome, started)
            if error is not None:
                raise error
            assert result is not None
            if not retry:
                break
            self.sleep(DOLT_RETRY_SLEEP_S)
        assert result is not None
        if check and result.returncode:
            raise CloseError("COMMAND.FAILED", result.stderr.strip() or result.stdout.strip(),
                             f"Repair {' '.join(argv[:3])} and rerun with the same vars file.")
        return result

    def _log_attempt(self, argv, attempt, began, outcome, started):
        """Record every non-clean attempt (and a success after a retry); main() prints them under "attempts"."""
        record = {"close_attempt": {"command": " ".join(argv[:3]), "attempt": attempt, "timeout_s": COMMAND_TIMEOUT,
                                    "elapsed_s": round(max(0, self.clock() - began), 3), "outcome": outcome,
                                    "retries_until_now": attempt - 1,
                                    "total_elapsed_s": round(max(0, self.clock() - started), 3), **self.sample()}}
        self.attempts.append(record["close_attempt"])

    def _run_once(self, argv, *, lead=False, git=False):
        try:
            return self.runner(list(argv), cwd=self.worktree if git else self.root,
                               env=self.lead_env if lead else self.env, capture_output=True, text=True,
                               timeout=COMMAND_TIMEOUT)
        except subprocess.TimeoutExpired as error:
            raise CloseError("COMMAND.FAILED", f"{list(argv)!r} timed out after {COMMAND_TIMEOUT}s",
                             f"Retry once, or raise {TIMEOUT_ENV} if the host is loaded, then rerun with the same vars file.") from error
        except (OSError, subprocess.SubprocessError) as error:
            raise CloseError("COMMAND.FAILED", str(error), f"Repair {argv[0]} and rerun with the same vars file.") from error

    def record_github_failure(self, argv, detail):
        # Each failed attempt overwrites this, so the result names the last one.
        self.github_error = f"{' '.join(argv)}: {str(detail).strip() or 'no output'}"

    def json(self, argv, **kwargs):
        self.last_json_argv = list(argv)
        try:
            data = json.loads(self.run(argv, **kwargs).stdout)
        except ValueError as error:
            producer = " ".join(argv)
            raise CloseError("COMMAND.INVALID", f"{producer} returned invalid JSON: {error}",
                             f"Repair the JSON returned by `{producer}` at the reported syntax location and retry.") from error
        return validate_command_json(argv, data)

    def read_bead(self, bead):
        rows = self.json(["bd", "show", bead, "--json"])
        if not isinstance(rows, list) or len(rows) != 1:
            raise CloseError("BEAD.INVALID", bead, "Supply an existing unique bead id.")
        return rows[0]

    def bead(self, bead):
        return self.records[bead]

    def events(self, task):
        return self.json(["atm", "task", "events", task, "--all", "--json"])["events"]

    def beads_directory(self):
        configured = self.env.get("BEADS_DIR")
        if configured:
            return configured
        location = self.json(["bd", "where", "--json"])
        path = location.get("path") if isinstance(location, dict) else None
        if not isinstance(path, str) or not Path(path).is_absolute():
            raise CloseError("BEADS_DIR.INVALID", repr(path), "Set BEADS_DIR or repair `bd where --json` to return an absolute beads path.")
        return path

    def snapshot(self, *, include_stack=False):
        values = self.values
        beads = {self.task}
        tasks = {self.task}
        if self.role in ("dev", "fix"):
            beads.add(values["sanity_bead"])
            if include_stack and values.get("stack_bead"):
                beads.add(values["stack_bead"])
            tasks.add(values["sanity_task_id"])
        elif self.role == "sanity":
            beads.add(values["checked_bead"])
            if values["verdict"] == "PASS":
                beads.add(values["qa_bead"])
                if values.get("outcome") != "not_reproducible":
                    beads.add(values["stack_bead"])
                tasks.add(values["qa_task_id"])
        self.records = {bead: self.read_bead(bead) for bead in sorted(beads)}
        self.task_history = {task: self.events(task) for task in sorted(tasks)}
        events = self.task_history[self.task]
        _, self.assignment_id, self.assignment_round = assignment_details(events, self.task)
        return self.check_replay()

    def cache(self):
        """The bd-cache every bead write goes through, so cache readers (close-vars, hooks) see it at once (#557)."""
        if self.beads is None:
            try:
                self.beads = bd_cache.BdCache(self.root, runner=self.cache_runner)
            except bd_cache.CacheError as error:
                raise CloseError(error.code, error.detail, error.fix) from error
        return self.beads

    def cache_runner(self, command, **_):
        """bd calls made by the cache keep this hook's environment, retries and attempt log."""
        try:
            return self.run(command, check=False)
        except CloseError as error:
            raise OSError(str(error)) from error

    def write_bead(self, bead, op, args):
        """Journal one bead write and drain it now; the cache stores bd's read-back row."""
        cache = self.cache()
        key = json.dumps([*self.key, self.assignment_id, bead, op, args], sort_keys=True)
        try:
            seq = cache.write(bead, op, args, idem_key=key, actor=self.identity)["seq"]
            for _ in range(DRAIN_TRIES):
                if not cache.drain()["busy"]:
                    break
                self.sleep(DRAIN_BUSY_SLEEP_S)
            entry = cache.entry(seq)
        except bd_cache.CacheError as error:
            raise CloseError(error.code, f"{bead} {op}: {error.detail}", error.fix) from error
        if entry["state"] == "applied":
            return
        if entry["state"] == "escalated":
            raise CloseError("BEADS.WRITE_ESCALATED", f"{bead} {op} (bd-cache journal #{seq}) was not applied: {entry['last_error']}",
                             f"Ask the phase lead to set {bead} in bd and run `bd_cache.py resolve {seq} --note <what> --actor <lead>`, "
                             "then rerun with the same vars file.")
        raise CloseError("BEADS.WRITE_PENDING", f"{bead} {op} is journaled (bd-cache #{seq}) but not applied yet: "
                         f"{entry['last_error'] or 'the drain is busy'}",
                         "Rerun with the same vars file once bd answers; the journaled write is applied once "
                         "(the bd-cache cron also applies it within a tick).")

    def update(self, bead, **fields):
        args = {}
        for name, value in fields.items():
            if name == "metadata":
                args["set_metadata"] = {key: bd_value(item) for key, item in value.items()}
            else:
                args[name] = str(value)
        self.write_bead(bead, "update", args)
        record = self.bead(bead)
        for name, value in fields.items():
            if name == "metadata":
                record["metadata"] = {**(record.get("metadata") or {}), **value}
            else:
                record[name] = value

    def metadata(self, bead, values):
        current = self.bead(bead).get("metadata") or {}
        changes = {key: value for key, value in values.items()
                   if (sanity_round_key(current, bead) if key == "sanity_round_key" else current.get(key)) != value}
        if changes:
            self.update(bead, metadata=changes)

    def check_replay(self):
        record = self.bead(self.task)
        metadata = record.get("metadata") or {}
        events = self.task_history[self.task]
        latest = events[-1] if events else {}
        if self.role == "sanity" and latest.get("event") == REFUSED:
            if self.values["verdict"] == "CANNOT_RUN":
                return True
            raise CloseError("REPLAY.CONFLICT", "sanity task was refused", "Request a new same-id assignment before PASS or FAIL.")
        count_key = sanity_round_key(metadata, self.task) if self.role == "sanity" else None
        if self.role == "sanity" and isinstance(count_key, list) and len(count_key) == 4:
            recorded = [count_key[0], count_key[2], count_key[3]]
            if self.values["round"] < self.assignment_round:
                if recorded == self.key:
                    return True
                raise CloseError("REPLAY.CONFLICT", "older assignment vars have a different verdict or sha", "Use the vars generated for the current task assignment.")
            if count_key[1] == self.assignment_id and recorded != self.key:
                raise CloseError("REPLAY.CONFLICT", "recorded verdict or sha differs", "Use the original vars for a replay or start a new review assignment.")
        if self.role in ("sanity", "qa") and record.get("status") == "closed":
            expected = f"{self.values['verdict']} at {self.values['commit']}"
            if record.get("close_reason") != expected:
                raise CloseError("REPLAY.CONFLICT", "recorded verdict or sha differs", "Use the original vars for a replay or start a new review assignment.")
        return False

    def assignment_is_current(self):
        _, assignment_id, _ = assignment_details(self.events(self.task), self.task)
        return assignment_id == self.assignment_id

    def stack_top(self):
        if not self.values.get("stack_bead"):
            branch = self.values.get("pr_target") or self.values.get("base")
            if not branch:
                raise CloseError("VARS.EMPTY", "pr_target", "Set pr_target to the checked layer's lower-bound branch.")
            head = self.run(["git", "rev-parse", f"origin/{branch}"], git=True).stdout.strip()
            if not head:
                raise CloseError("STACK.TOP", branch, f"Fetch and verify origin/{branch} before retrying.")
            return branch, head
        self.last_json_argv = ["bd", "show", self.values["stack_bead"], "--json"]
        return select_stack_top(
            self.bead(self.values["stack_bead"]), self.values["pr_target"],
            lambda branch: self.run(["git", "rev-parse", f"origin/{branch}"], git=True).stdout.strip(),
        )

    def checks(self):
        values = self.values
        head = values["commit"]
        dirty = self.run(["git", "status", "--porcelain"], git=True).stdout.strip()
        if dirty:
            raise CloseError("HEAD.DIRTY", dirty, "Commit or remove worktree changes before retrying.")
        remote_available = False
        remote_ref = f"refs/heads/{values['branch']}"
        argv = ["git", "ls-remote", "origin", remote_ref]
        for _attempt in range(3):
            try:
                argv = ["git", "ls-remote", "origin", remote_ref]
                remote = self.run(argv, git=True, check=False)
                if remote.returncode:
                    self.record_github_failure(argv, remote.stderr)
                    continue
                remote_fields = remote.stdout.split()
                if not remote_fields or remote_fields[0] != head:
                    raise CloseError("HEAD.NOT_PUSHED", "origin head differs from HEAD", "Push this branch and retry.")
                argv = ["git", "fetch", "origin"]
                fetched = self.run(argv, git=True, check=False)
                if fetched.returncode:
                    self.record_github_failure(argv, fetched.stderr)
                    continue
                remote_available = True
                break
            except CloseError as error:
                if error.code != "COMMAND.FAILED":
                    raise
                self.record_github_failure(argv, error)
        if not remote_available:
            self.github = "unavailable"
            tracking_ref = f"refs/remotes/origin/{values['branch']}"
            tracking = self.run(["git", "rev-parse", tracking_ref], git=True).stdout.strip()
            if tracking != head:
                raise CloseError("HEAD.NOT_PUSHED", "cached origin head differs from HEAD",
                                 "Push this branch and refresh origin before retrying.")
        top, top_head = self.stack_top()
        values["rebased_onto"] = top_head
        ancestor = self.run(["git", "merge-base", "--is-ancestor", f"origin/{top}", head], git=True, check=False)
        if ancestor.returncode == 1:
            raise CloseError("BASE.NOT_ANCESTOR", top, f"Rebase onto origin/{top}, push, then rerun the hook.")
        if ancestor.returncode:
            raise CloseError("COMMAND.FAILED", ancestor.stderr, "Repair the local git ancestry check.")
        remote_url = self.run(["git", "remote", "get-url", "origin"], git=True).stdout.strip()
        match = re.search(r"[:/]([^/:]+)/([^/]+?)(?:\.git)?$", remote_url)
        if not match:
            raise CloseError("COMMAND.INVALID", "origin does not name owner/repository", "Configure the repository origin.")
        owner, repo = match.groups()
        if self.github == "unavailable":
            self.keep_recorded_pr()
            return
        for _attempt in range(3):
            try:
                prs = self.json(["gh", "api", f"repos/{owner}/{repo}/pulls?head={owner}:{values['branch']}&state=open"], git=True)
            except CloseError as error:
                if error.code not in ("COMMAND.FAILED", "COMMAND.INVALID"):
                    raise
                self.record_github_failure(self.last_json_argv, error)
                continue
            matches = [pr for pr in prs if pr["head"]["sha"] == head]
            if not matches:
                raise CloseError("PR.NOT_FOUND", "no open PR for this branch at HEAD", "Open the PR at the pushed head, then rerun the hook.")
            if len(matches) != 1:
                raise CloseError("PR.AMBIGUOUS", "several open PRs match this branch at HEAD",
                                 "Close the duplicate PRs so exactly one open PR names this branch and pushed head, then rerun the hook.")
            values["pr_number"] = str(matches[0]["number"] or "")
            values["pr_url"] = f"https://github.com/{owner}/{repo}/pull/{values['pr_number']}" if values["pr_number"] else ""
            return
        self.github = "unavailable"
        self.keep_recorded_pr()

    def keep_recorded_pr(self):
        # Without GitHub the PR identity is the one the bead already records.
        metadata = self.bead(self.task).get("metadata") or {}
        self.values["pr_number"] = str(metadata.get("pr_number") or "")
        self.values["pr_url"] = str(metadata.get("pr_url") or "")

    def render(self, template, values=None):
        try:
            return sc_render.render(TEMPLATES / template, self.values if values is None else values)
        except sc_render.RenderError as error:
            raise CloseError("VARS.INVALID", str(error), f"Supply vars compatible with {template}.") from error

    def send(self, recipient, template):
        self.run(["atm", "send", recipient, "--template", str(TEMPLATES / template), "--vars", str(self.path)])

    def assign(self, member, bead, template, *, path=None, due=None):
        events = self.task_history[bead]
        if due is None:
            due = not events or (is_terminal(events) and self.bead(bead).get("status") == "open")
        if due:
            if template == "dev-sanity-template.xml.j2":
                context = read_vars(Path(path or self.path))
                with tempfile.TemporaryDirectory(prefix="sanity-dispatch-") as directory:
                    dispatch = Path(directory) / "completion-vars.json"
                    close_context = {**context, "phase_lead": self.lead,
                                     "budget": context.get("budget", "full")}
                    dispatch.write_text(json.dumps({**context, "close_context": close_context,
                                                    "beads_dir": self.beads_directory()}))
                    self.render(template, read_vars(dispatch))
                    self.run(["atm", "task", "assign", member, "--task-id", bead, "--template", str(TEMPLATES / template),
                              "--vars", str(dispatch)], lead=True)
            else:
                self.run(["atm", "task", "assign", member, "--task-id", bead, "--template", str(TEMPLATES / template),
                          "--vars", str(path or self.path)], lead=True)
        self.assigned.append(bead)

    def close_task(self, task, template=None, *, outcome=COMPLETED, lead=False, reason=None):
        events = self.task_history[task]
        if not is_terminal(events):
            argv = ["atm", "task", "close", task, outcome]
            if template:
                argv += ["--template", str(TEMPLATES / template), "--vars", str(self.path)]
            if reason:
                argv.append(reason)
            self.run(argv, lead=lead)

    def close_bead(self, bead, reason):
        record = self.bead(bead)
        wrote = False
        if record.get("status") != "closed":
            self.write_bead(bead, "close", {"reason": reason})
            record.update(status="closed", close_reason=reason)
            wrote = True
        self.closed.append(bead)
        return wrote

    def derive_local(self):
        # The completion fields the fixer used to type come from the worktree and the open PR instead.
        head = self.run(["git", "rev-parse", "HEAD"], git=True).stdout.strip()
        self.values.update(commit=head, generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                           pr_number="", pr_url="", rebased_onto="")
        self.require_completion_identity(("commit",))
        self.key = [self.task, self.values.get("verdict", ""), head]

    def verify_not_reproducible_evidence(self):
        evidence = str(self.values.get("evidence", ""))
        match = re.fullmatch(r"(.+):([1-9][0-9]*)", evidence)
        if not match:
            raise CloseError("EVIDENCE.INVALID", evidence, "Supply a repo-relative file:line citation.")
        relative, line = match.group(1), int(match.group(2))
        path = Path(relative)
        target = (self.worktree / path).resolve()
        if path.is_absolute() or ".." in path.parts or not target.is_relative_to(self.worktree):
            raise CloseError("EVIDENCE.INVALID", evidence, "Cite a file inside the assigned worktree.")
        try:
            lines = target.read_text(encoding="utf-8").splitlines()
        except (OSError, UnicodeDecodeError) as error:
            raise CloseError("EVIDENCE.INVALID", evidence, f"Restore the cited file in {self.worktree}: {error}") from error
        if line > len(lines):
            raise CloseError("EVIDENCE.INVALID", evidence, f"Cited line {line} does not exist in {relative} ({len(lines)} lines).")

    def require_completion_identity(self, fields=("commit", "pr_number", "pr_url")):
        if self.values["outcome"] == "not_reproducible":
            return
        for name in fields:
            if not str(self.values.get(name) or "").strip():
                raise CloseError("VARS.EMPTY", f"derived {name} is empty",
                                 "Push a non-empty HEAD and open exactly one PR; when GitHub is unavailable or checks are skipped, record its pr_number and pr_url on the checked bead before retrying.")

    def dev(self, skip):
        values = self.values
        self.derive_local()
        if self.snapshot(include_stack=not skip or (values["outcome"] == "not_reproducible" and bool(values.get("stack_bead")))):
            return
        template = "fix-complete.md.j2" if self.role == "fix" else "dev-complete.md.j2"
        record = self.bead(values["checked_bead"])
        sanity = (record.get("metadata") or {}).get("sanity_bead")
        if not sanity:
            candidates = self.json(["bd", "list", "-l", "stage:dev-sanity", "--status", "open", "-n", "0", "--json"])
            matches = [row["id"] for row in candidates if (row.get("metadata") or {}).get("dev_bead") == values["checked_bead"]]
            sanity = matches[0] if len(matches) == 1 else None
        if sanity != values["sanity_bead"]:
            raise CloseError("BEAD.NO_SANITY", "no matching group sanity bead", f"Ask {self.lead} to create or correct the group sanity bead.")
        with tempfile.TemporaryDirectory(prefix="atm-task-close-") as directory:
            completion = Path(directory) / "completion-vars.json"
            if values["outcome"] == "not_reproducible":
                branch, commit = self.stack_top()
                assignment = {**values, "branch": branch, "commit": commit,
                              "close_context": {**values, "phase_lead": self.lead},
                              "beads_dir": self.beads_directory()}
                self.render("dev-sanity-template.xml.j2", assignment)
                assignment_path = Path(directory) / "sanity-vars.json"
                assignment_path.write_text(json.dumps(assignment), encoding="utf-8")
                self.render(template)
                completion.write_text(json.dumps(values), encoding="utf-8")
                self.path = completion
                self.finish_dev(sanity, template, path=assignment_path)
                return
            if not skip:
                self.checks()
            else:
                self.keep_recorded_pr()
                values["rebased_onto"] = self.run(["git", "rev-parse", f"origin/{values['pr_target']}"], git=True).stdout.strip()
            self.require_completion_identity()
            self.render(template)
            completion.write_text(json.dumps(values), encoding="utf-8")
            self.path = completion
            self.finish_dev(sanity, template)

    def finish_dev(self, sanity, template, *, path=None):
        values = self.values
        self.require_completion_identity()
        assignment_values = json.loads(Path(path).read_text()) if path else values
        sanity_events = self.task_history[sanity]
        terminal = is_terminal(sanity_events)
        open_assignment = bool(sanity_events) and not terminal
        checked_metadata = self.bead(values["checked_bead"]).get("metadata") or {}
        sha_changed = checked_metadata.get("commit") != assignment_values["commit"]
        assign = (not terminal or self.bead(sanity).get("status") == "open") and (sha_changed or not open_assignment)
        if not assign and sha_changed:
            raise CloseError("REPLAY.CONFLICT", "sanity is closed at a different sha", "Request a new sanity assignment before changing the checked sha.")
        self.metadata(values["checked_bead"], {"commit": assignment_values["commit"], "branch": assignment_values["branch"], "pr_number": values.get("pr_number", ""),
                                             "pr_url": values["pr_url"]})
        reason = f"not reproducible: {values['evidence']}" if values["outcome"] == "not_reproducible" else f"{'fixed' if self.role == 'fix' else 'dev complete'} at {values['commit']}"
        self.close_bead(values["checked_bead"], reason)
        if assign and not open_assignment:
            self.send(self.lead, template)
        self.assign(self.config["dev_sanity_member"], sanity, "dev-sanity-template.xml.j2", path=path, due=assign)
        # The dev's work is finished: free the dev for its next queued task and stop reminders.
        # Sanity state lives on the sanity task; a FAIL re-assigns this task through the lead.
        self.close_task(self.task, reason=reason)

    def sanity(self):
        values = self.values
        bead = values["sanity_bead"]
        verdict = values["verdict"]
        if self.snapshot():
            return
        if verdict == "CANNOT_RUN":
            self.render("dev-sanity-complete.md.j2")
            record = self.bead(bead)
            if record.get("status") != "open" or record.get("assignee"):
                self.update(bead, status="open", assignee="")
            self.close_task(bead, "dev-sanity-complete.md.j2", outcome=REFUSED)
            return
        self.render("dev-sanity-complete.md.j2")
        if verdict == "PASS":
            checked = self.bead(values["checked_bead"])
            is_fix = (checked.get("metadata") or {}).get("role") == "fix" or "stage:fix" in checked.get("labels", [])
            self.render("qa-template.xml.j2")
            if is_fix and values.get("carry_forward", "") != values["checked_bead"]:
                raise CloseError("VARS.INVALID", "fix QA requires carry_forward", "Supply the checked fix bead as carry_forward.")
            if values.get("carry_forward") and values["carry_forward"] != values["checked_bead"]:
                raise CloseError("VARS.INVALID", "carry_forward differs from fix bead", "Supply the checked fix bead as carry_forward.")
            self.close_bead(bead, f"PASS at {values['commit']}")
            self.close_task(bead, "dev-sanity-complete.md.j2")
            self.assign(self.config["qa_member"], values["qa_bead"], "qa-template.xml.j2")
            return
        self.render("sanity-fail.xml.j2")
        record = self.bead(bead)
        metadata = record.get("metadata") or {}
        count_key = [self.task, self.assignment_id, values["verdict"], values.get("commit", "")]
        count = sanity_round(metadata, bead)
        prior_key = sanity_round_key(metadata, bead)
        if prior_key != count_key:
            count += 1
            self.metadata(bead, {"sanity_round": count, "sanity_round_key": count_key})
            if not self.assignment_is_current():
                return
        if count > self.config["max_sanity_rounds"]:
            events = self.task_history[bead]
            if not is_terminal(events):
                message = f"SANITY.ROUND_CAP {bead} {values['commit']} assignment={self.assignment_id} undone=" + ",".join(str(item["number"]) for item in values["undone"])
                self.run(["atm", "send", self.config["team_lead_member"], message])
            self.close_task(bead, "dev-sanity-complete.md.j2")
            return "SANITY.ROUND_CAP"
        checked = self.bead(values["checked_bead"])
        if checked.get("status") != "open":
            self.update(values["checked_bead"], status="open", assignee=checked["assignee"])
        if record.get("status") != "open" or record.get("assignee"):
            self.update(bead, status="open", assignee="")
        events = self.task_history[bead]
        if not is_terminal(events):
            self.send(checked["assignee"], "sanity-fail.xml.j2")
        self.close_task(bead, "dev-sanity-complete.md.j2")

    def qa(self):
        values = self.values
        if self.snapshot():
            return
        self.render("qa-complete.md.j2")
        self.close_bead(values["qa_bead"], f"{values['verdict']} at {values['commit']}")
        events = self.task_history[values["qa_bead"]]
        if not is_terminal(events):
            message = f"QA {values['verdict']} {values['qa_bead']} commit={values['commit']} round={values['round']}"
            self.run(["atm", "send", self.lead, message])
        self.close_task(values["qa_bead"], "qa-complete.md.j2")


def main(argv=None, runner=subprocess.run, clock=time.monotonic, sleep=time.sleep, sample=load_sample):
    parser = Parser(description=__doc__)
    parser.add_argument("--role", required=True, choices=ROLES)
    parser.add_argument("--vars", required=True, type=Path)
    parser.add_argument("--result", type=Path, help="the fixer's result file; required for dev and fix")
    parser.add_argument("--skip-checks", action="store_true", help="skips the PR and stack checks. Do NOT use without permission from the phase lead.")
    hook = None
    args = None
    output: dict[str, object]
    try:
        args = parser.parse_args(argv)
        try:
            command_timeout()
        except ValueError as error:
            raise CloseError("CONFIG.INVALID", str(error), f"Unset {TIMEOUT_ENV} or set it to a positive integer number of seconds, e.g. {TIMEOUT_ENV}=180.") from error
        values = read_vars(args.vars)
        if args.role in ("dev", "fix"):
            if args.result is None:
                raise CloseError("ARGS.INVALID", "--result is required for dev and fix",
                                 "Pass the fixer's result file with --result <result-file>.")
            values = merge_result(values, read_vars(args.result), args.role)
        validate_shared_vars(values, args.role)
        hook = Hook(args.role, values, args.vars, runner, clock, sleep, sample)
        if args.role == "fix" and values.get("outcome") == "not_reproducible":
            hook.verify_not_reproducible_evidence()
        started = clock()
        escalated = hook.dev(args.skip_checks) if args.role in ("dev", "fix") else hook.sanity() if args.role == "sanity" else hook.qa()
        result = {"role": args.role, "bead": hook.task, "commit": values.get("commit", ""), "pr_number": values.get("pr_number", ""),
                  "github": hook.github, "assigned": hook.assigned, "closed": hook.closed, "elapsed_s": max(0, clock() - started)}
        if hook.github == "unavailable":
            result["github_error"] = hook.github_error
        if escalated:
            result["escalated"] = escalated
        output = {"ok": result}
    except CloseError as error:
        output = {"err": {"code": error.code, "message": str(error), "fix": error.fix}}
    except KeyError as error:
        field = str(error.args[0])
        producer = " ".join(hook.last_json_argv) if hook is not None else str(args.vars if args is not None else "--vars")
        output = {"err": {"code": "COMMAND.INVALID", "message": f"{producer}: required field {field} is missing",
                          "fix": f"Restore field {field} in `{producer}` before retrying."}}
    except OSError as error:
        path = str(error.filename or "the path named in the error")
        output = {"err": {"code": "COMMAND.FAILED", "message": str(error),
                          "fix": f"Restore file access for {path} and rerun with the same vars file."}}
    except (ValueError, TypeError) as error:
        producer = " ".join(hook.last_json_argv) if hook is not None else str(args.vars if args is not None else "--vars")
        output = {"err": {"code": "COMMAND.INVALID", "message": f"{producer}: {type(error).__name__}: {error}",
                          "fix": f"Repair the invalid value reported by {type(error).__name__} in `{producer}`: {error}."}}
    if hook is not None and hook.attempts:
        output["attempts"] = hook.attempts
    print(json.dumps(output))
    print(json.dumps(output), file=sys.stderr)
    return 1 if "err" in output else 0


if __name__ == "__main__":
    raise SystemExit(main())
