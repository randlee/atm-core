#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.12"
# dependencies = ["pydantic>=2", "pyyaml", "sc-compose>=1.8.0,<2"]
# ///
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
from datetime import datetime, timezone

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import bd_cache
import close_fields
import phase_settings
import repo_config
import sc_render
from command_timeout import TIMEOUT_ENV, command_timeout, import_timeout, repo_timeout
from stack_top import select_stack_top
from task_close_validation import (ASSIGNMENT_EVENTS, CloseError, assignment_details, assignment_id_for_round,
                                   sanity_round, sanity_round_key, validate_command_json, header, reviewer_list, validate,
                                   sanity_close_mode, bd_value, read_vars, merge_result, validate_outcome,
                                   read_toml_setting, read_config)

TEMPLATES = Path(__file__).resolve().parents[1] / "templates"
ROLES = ("dev", "fix", "sanity", "qa")
COMMAND_TIMEOUT = import_timeout()
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
# ATM's HTTP request body limit: the ATM daemon's DEFAULT_MAX_MESSAGE_BYTES (1,048,576) plus
# CANONICAL_WRITE_ENVELOPE_OVERHEAD_BYTES (65,536), crates/atm-daemon-bootstrap/src/lib.rs.
ATM_MAX_REQUEST_BYTES = 1_114_112
# The dev/fix fields the sanity close reads back from close_context: sanity-close-vars build() and
# write_stack_input(), then atm-task-close.py --role sanity and the templates it renders and sends
# (dev-sanity-complete, sanity-fail, qa-template). Every other field is overwritten there or never read.
SANITY_CLOSE_CONTEXT = frozenset((
    "base", "base_verified", "base_source", "nr", "carry_forward", "checked_title", "commit", "evidence", "layer", "layer_prs", "max_sanity_rounds", "outcome",
    "phase", "phase_feature", "policy_path", "pr_number", "pr_url", "primary_checkout", "qa_bead", "qa_task_id",
    "reviewers_round1", "round", "sprint_bead", "stack", "stack_bead"))






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






class Parser(argparse.ArgumentParser):
    def error(self, message):
        raise CloseError("ARGS.INVALID", message, "Supply --role dev|fix|sanity|qa and --vars <shared-vars.json>.")










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
        try:
            team = phase_settings.phase_settings(self.root, values["phase"]).team
        except phase_settings.PhaseSettingsError as error:
            raise CloseError("CONFIG.INVALID", str(error), str(error)) from error
        self.env = dict(os.environ)
        self.lead_env = {**self.env, "ATM_IDENTITY": self.lead, "ATM_TEAM": team}
        self.github = "ok"
        # The PR's actual base, recorded on the checked bead for its QA assignment; not a vars field.
        self.pr_base = ""
        self.github_error = ""
        self.closed, self.assigned = [], []
        self.task = values[{"dev": "dev_task_id", "fix": "dev_task_id", "sanity": "sanity_task_id", "qa": "qa_task_id"}[role]]
        self.key = [self.task, values.get("verdict", ""), values.get("commit", "")]
        self.records, self.task_history = {}, {}
        self.attempts = []
        self.beads = None
        self.assignment_id = None
        self.beads_dir = ""
        self.ledger_top = None
        self.base_evidence = None
        self.check_record = {}
        self.rebased_onto = ""
        self.base_difference = ""

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
            env = self.lead_env if lead else self.env
            if argv[:3] == ["bd", "where", "--json"]:
                env = {key: value for key, value in env.items() if key != "BEADS_DIR"}
            return self.runner(list(argv), cwd=self.worktree if git else self.root,
                               env=env, capture_output=True, text=True,
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
        location = self.json(["bd", "where", "--json"])
        path = location.get("path") if isinstance(location, dict) else None
        if not isinstance(path, str) or not Path(path).is_absolute():
            raise CloseError("BEADS_DIR.INVALID", repr(path), "Repair repository configuration so `bd where --json` returns an absolute beads path.")
        self.beads_dir = path
        self.env["BEADS_DIR"] = path
        self.lead_env["BEADS_DIR"] = path
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
        try:
            _, self.assignment_id, self.assignment_round = assignment_details(events, self.task)
        except CloseError as error:
            if self.role not in ("dev", "fix"):
                raise
            latest = events[-1] if events else {}
            raise CloseError("CLOSE.STALE_ASSIGNMENT", f"{self.task}: no live assignment; newest event {latest.get('message_id') or latest.get('seq') or latest.get('event') or 'none'}",
                             f"Run `atm task events {self.task} --json` and request an assignment before closing.") from error
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
        if self.role in ("dev", "fix"):
            events = self.events(self.task)
            assignment, current, _ = assignment_details(events, self.task)
            if current != self.assignment_id or is_terminal(events) or (assignment.get("assignee") and assignment["assignee"] != self.identity):
                raise CloseError("CLOSE.STALE_ASSIGNMENT", f"{self.task}: assignment {self.assignment_id} is stale; current assignment is {current}, newest event is {events[-1].get('event')}",
                                 f"Run `atm task events {self.task} --json` and close under the current assignment.")
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
        if self.role in ("dev", "fix"):
            assignment, _, _ = assignment_details(events, self.task)
            assignee = assignment.get("assignee")
            if assignee and assignee != self.identity:
                raise CloseError("CLOSE.STALE_ASSIGNMENT", f"{self.task}: assignment {self.assignment_id} belongs to {assignee}, not {self.identity}",
                                 f"Run `atm task events {self.task} --json` and close as the current assignee.")
            previous_generation = metadata.get("close_assignment_id")
            previous_close = ((not previous_generation or str(previous_generation) != str(self.assignment_id))
                              and any(event.get("event") == COMPLETED for event in events[:-1]))
            prior_commits = {metadata.get("commit")} if previous_close and metadata.get("commit") else set()
            if record.get("status") != "closed":
                for event in events:
                    detail = event.get("detail")
                    if event.get("event") != COMPLETED or not isinstance(detail, str):
                        continue
                    prefix = next((value for value in ("dev complete at ", "fixed at ") if detail.startswith(value)), None)
                    if prefix is None:
                        continue
                    sha = detail[len(prefix):]
                    if not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", sha):
                        event_id = event.get("message_id") or event.get("seq") or "unknown"
                        raise CloseError("CLOSE.HISTORY_MALFORMED", f"{self.task} event {event_id}: malformed close commit in {detail!r}",
                                         f"Inspect `atm task events {self.task} --all --json` and reconcile the bead's close history before retrying.")
                    prior_commits.add(sha)
            if record.get("status") != "closed" and self.values["commit"] in prior_commits:
                raise CloseError("CLOSE.STALE_ASSIGNMENT", f"{self.task}: prior close commit {self.values['commit']} was reopened under assignment {self.assignment_id}",
                                 f"Push a rework commit with `git push origin {self.values['branch']}` and re-close; inspect `atm task events {self.task} --json`.")
            if is_terminal(events):
                expected_reason = (f"not reproducible: {self.values['evidence']}" if self.values["outcome"] == "not_reproducible"
                                   else f"{'fixed' if self.role == 'fix' else 'dev complete'} at {self.values['commit']}")
                exact = (latest.get("event") == COMPLETED and metadata.get("close_assignment_id") == str(self.assignment_id)
                         and metadata.get("commit") == self.values["commit"]
                         and record.get("status") == "closed"
                         and record.get("close_reason") == expected_reason)
                if exact:
                    sanity = self.values["sanity_task_id"]
                    self.verify_sanity_assignment(sanity, self.task_history[sanity], metadata.get("sanity_assignment_id"), allow_pass=True)
                    return True
                raise CloseError("CLOSE.STALE_ASSIGNMENT", f"{self.task}: newest event {latest.get('message_id') or latest.get('seq') or latest.get('event')} is terminal; current assignment is {self.assignment_id}",
                                 f"Run `atm task events {self.task} --json` and request a new assignment before closing.")
        if self.role == "sanity" and latest.get("event") == REFUSED:
            if self.values["verdict"] == "CANNOT_RUN":
                return True
            raise CloseError("REPLAY.CONFLICT", "sanity task was refused", "Request a new same-id assignment before PASS or FAIL.")
        count_key = sanity_round_key(metadata, self.task) if self.role == "sanity" else None
        if self.role == "sanity" and self.values["round"] < self.assignment_round:
            prior_assignment = assignment_id_for_round(events, self.task, self.values["round"])
            if (isinstance(count_key, list) and len(count_key) == 4
                    and [count_key[0], count_key[2], count_key[3]] == self.key
                    and count_key[1] == prior_assignment and prior_assignment is not None):
                return True
            raise CloseError("REPLAY.CONFLICT", "sanity round is stale",
                             "Use the current sanity assignment round, or replay the exact recorded prior verdict and commit.")
        if self.role == "sanity" and isinstance(count_key, list) and len(count_key) == 4:
            recorded = [count_key[0], count_key[2], count_key[3]]
            if count_key[1] == self.assignment_id and recorded != self.key:
                raise CloseError("REPLAY.CONFLICT", "recorded verdict or sha differs", "Use the original vars for a replay or start a new review assignment.")
        if self.role in ("sanity", "qa") and record.get("status") == "closed" and is_terminal(events):
            expected = f"{self.values['verdict']} at {self.values['commit']}"
            if record.get("close_reason") != expected:
                raise CloseError("REPLAY.CONFLICT", "recorded verdict or sha differs", "Use the original vars for a replay or start a new review assignment.")
        return False

    def assignment_is_current(self):
        _, assignment_id, _ = assignment_details(self.events(self.task), self.task)
        return assignment_id == self.assignment_id

    def verify_sanity_assignment(self, sanity, events, expected, *, allow_pass=False):
        """Check the recorded generation, recipient and live state, allowing only a matching completed PASS on replay."""
        assignments = [event for event in events if event.get("event") in ASSIGNMENT_EVENTS]
        assignment = assignments[-1] if assignments else {}
        actual = assignment.get("message_id") or assignment.get("seq")
        state = events[-1].get("to_state") if events else None
        recipient = assignment.get("assignee")
        latest_recipient = events[-1].get("assignee") if events else None
        member = self.config["dev_sanity_member"].split("@", 1)[0]
        same = (expected is not None and actual is not None and str(expected) == str(actual)
                and isinstance(recipient, str) and recipient.split("@", 1)[0] == member
                and (latest_recipient is None or (isinstance(latest_recipient, str)
                     and latest_recipient.split("@", 1)[0] == member)))
        passed = (allow_pass and events and events[-1].get("event") == COMPLETED
                  and self.bead(sanity).get("status") == "closed"
                  and self.bead(sanity).get("close_reason") == f"PASS at {self.values['commit']}")
        if not same or (state not in ("assigned", "active") and not passed):
            detail = f"{sanity}: expected generation {expected!r}, recipient {member}, assigned or active"
            observed = f"observed generation {actual!r}, recipient {recipient!r}, state {state!r}, event {events[-1].get('event') if events else None!r}, at {assignment.get('at')!r}"
            fix = (f"Inspect `atm task events {sanity} --all --json`; the lead must verify and record the assignment with "
                   f"`bd update {self.values['checked_bead']} --set-metadata sanity_assignment_id=<verified-id>` or cancel the orphan and reassign, then retry.")
            raise CloseError("CLOSE.POSTCONDITION", f"{detail}; {observed}", fix)
        return str(actual)

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
        checks: dict[str, object] = {"beads_dir": self.beads_dir}
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
        all_layers = ((self.bead(values["stack_bead"]).get("metadata") or {}).get("layers") or []) if values.get("stack_bead") else []
        layers = [layer for layer in all_layers if layer.get("linked") is True]
        if layers:
            layer = layers[-1]
            self.ledger_top = {"branch": layer["branch"], "pr": layer.get("pr"), "head": layer["head"]}
        checks["ledger_top"] = self.ledger_top
        linked_index = next((index for index, layer in enumerate(layers) if layer.get("branch") == values["branch"]), None)
        self.linked_reclose = linked_index is not None
        if self.linked_reclose:
            assert linked_index is not None
            if layers[linked_index].get("head") != head:
                raise CloseError("BASE.LINKED_HEAD", f"linked layer {values['branch']} has a different ledger head",
                                 "Inspect the frozen layer and its ledger entry with the phase lead; do not rebase it.")
            if linked_index:
                parent = layers[linked_index - 1]
                self.base_anchor = (parent["branch"], parent["head"])
            else:
                stack = self.bead(values["stack_bead"]).get("metadata") or {}
                trunk = stack.get("trunk") or values["pr_target"]
                trunk_head = self.run(["git", "rev-parse", f"origin/{trunk}"], git=True).stdout.strip()
                self.base_anchor = (trunk, trunk_head)
        else:
            self.base_anchor = (top, top_head)
            ancestor = self.run(["git", "merge-base", "--is-ancestor", f"origin/{top}", head], git=True, check=False)
            if ancestor.returncode == 1:
                raise CloseError("BASE.NOT_ANCESTOR", top, f"Rebase onto origin/{top}, push, then rerun the hook.")
            if ancestor.returncode:
                raise CloseError("COMMAND.FAILED", ancestor.stderr, "Repair the local git ancestry check.")
        self.rebased_onto = self.base_anchor[1]
        remote_url = self.run(["git", "remote", "get-url", "origin"], git=True).stdout.strip()
        match = re.search(r"[:/]([^/:]+)/([^/]+?)(?:\.git)?$", remote_url)
        if not match:
            raise CloseError("COMMAND.INVALID", "origin does not name owner/repository", "Configure the repository origin.")
        owner, repo = match.groups()
        if self.github == "unavailable":
            self.keep_recorded_pr()
            self.require_completion_identity()
            self.resolve_recorded_pr_base()
            checks["pr_base"] = self.base_evidence
            self.check_record = checks
            return checks
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
            self.require_completion_identity()
            self.pr_base = str((matches[0].get("base") or {}).get("ref") or "")
            self.resolve_recorded_pr_base()
            base_layer = next((layer for layer in all_layers if layer.get("branch") == self.pr_base), {})
            self.base_evidence = {"branch": self.pr_base, "pr": base_layer.get("pr")}
            if self.ledger_top and self.ledger_top["branch"] != self.pr_base:
                self.base_difference = f"ledger top {self.ledger_top['branch']} (#{self.ledger_top['pr']}) differs from PR base {self.pr_base}"
                if self.base_evidence["pr"] is not None:
                    self.base_difference += f" (#{self.base_evidence['pr']})"
            checks["pr_base"] = self.base_evidence
            if self.base_difference:
                checks["base_difference"] = self.base_difference
            self.check_record = checks
            return checks
        self.github = "unavailable"
        self.keep_recorded_pr()
        self.require_completion_identity()
        self.resolve_recorded_pr_base()
        checks["pr_base"] = self.base_evidence
        self.check_record = checks
        return checks

    def keep_recorded_pr(self):
        # Without GitHub the PR identity is the one the bead already records.
        metadata = self.bead(self.task).get("metadata") or {}
        self.values["pr_number"] = str(metadata.get("pr_number") or "")
        self.values["pr_url"] = str(metadata.get("pr_url") or "")
        self.pr_base = str(metadata.get("pr_base") or "")

    def resolve_recorded_pr_base(self, *, skip_stack=False):
        """Resolve either PR parent and verify the ledger top, base and checked head form a chain."""
        bead = self.values["checked_bead"]
        base = self.pr_base
        correction = ("Retarget the PR to its recorded ledger predecessor; do not rebase the frozen layer"
                      if getattr(self, "linked_reclose", False) else "Retarget the PR to the ledger top or rebase onto it")
        fix = (f"{correction}, run `git fetch origin`, and if offline update {bead} with "
               f"`bd update {bead} --set-metadata pr_base=<actual-base-branch>`, then retry.")
        if not base:
            raise CloseError("BASE.PR_UNVERIFIED", f"{bead} metadata.pr_base is missing", fix)
        ref = f"origin/{base}"
        result = self.run(["git", "rev-parse", "--verify", f"{ref}^{{commit}}"], git=True, check=False)
        sha = result.stdout.strip()
        if result.returncode or not re.fullmatch(r"[0-9a-f]{40}", sha):
            raise CloseError("BASE.PR_UNVERIFIED", f"{bead} recorded PR base {ref} cannot be resolved to a commit", fix)
        lower = f"origin/{self.values['pr_target']}"
        top, top_head = ("unchecked", "") if skip_stack else (getattr(self, "base_anchor", None) or self.stack_top())
        if getattr(self, "linked_reclose", False) and sha != top_head:
            raise CloseError("BASE.PR_UNVERIFIED", f"{bead} PR base {ref} ({sha}) differs from its ledger predecessor {top} ({top_head})", fix)
        detail = f"{bead} PR base {ref} ({sha}) and ledger anchor {top} ({top_head})"
        self.verify_sanity_base(lower, sha, fix=fix, detail=detail, require_sha=False)
        if not skip_stack:
            self.verify_sanity_base(top_head, sha, fix=fix, detail=detail)
        self.verify_sanity_base(sha, self.values["commit"], fix=fix, detail=detail)
        self.rebased_onto = sha
        self.values["base_verified"] = not skip_stack
        self.base_evidence = {"branch": base, "pr": None}

    def verify_sanity_base(self, base, head, *, fix=None, detail="", require_sha=True):
        fix = fix or "Run `git fetch origin`, then ask the dispatcher to select a full 40-hex ancestor base and rerun the sanity assignment."
        if require_sha and (not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base)):
            raise CloseError("BASE.PR_UNVERIFIED", f"base {base!r} for checked head {head} is not a full 40-hex commit", fix)
        result = self.run(["git", "merge-base", "--is-ancestor", base, head], git=True, check=False)
        if result.returncode:
            prefix = f"{detail}: " if detail else ""
            raise CloseError("BASE.PR_UNVERIFIED", f"{prefix}{base} is not a verified ancestor of {head}", fix)

    def resolve_nr_base(self, *, skip_checks=False):
        values = self.values
        layers = []
        if values.get("stack_bead"):
            layers = [layer for layer in ((self.bead(values["stack_bead"]).get("metadata") or {}).get("layers") or [])
                      if layer.get("linked") is True]
        index = next((i for i, layer in enumerate(layers) if layer.get("branch") == self.checked_worktree_branch), None)
        source = "ledger" if index is not None else "pr_target"
        if index is not None and layers[index].get("head") != self.checked_worktree_head:
            raise CloseError("BASE.PR_UNVERIFIED", f"linked branch {self.checked_worktree_branch} has a different ledger head than {self.checked_worktree_head}",
                             "Ask the lead to inspect the frozen ledger entry; do not rebase it.")
        if index is not None and index > 0:
            sha = layers[index - 1].get("head")
        else:
            branch = ((self.bead(values["stack_bead"]).get("metadata") or {}).get("trunk")
                      if index is not None else values.get("pr_target"))
            ref = f"origin/{branch}"
            result = self.run(["git", "rev-parse", "--verify", f"{ref}^{{commit}}"], git=True, check=False)
            if result.returncode:
                raise CloseError("BASE.PR_UNVERIFIED", f"cannot resolve {ref} for checked head {self.checked_worktree_head}",
                                 "Fetch the correct base commit and ask the dispatcher to retry.")
            sha = result.stdout.strip()
        self.verify_sanity_base(sha, self.checked_worktree_head)
        values.update(rebased_onto=sha, base_source=source, nr=True, base_verified=not skip_checks)

    def render(self, template, values=None):
        try:
            return sc_render.render(TEMPLATES / template, self.values if values is None else values)
        except sc_render.RenderError as error:
            raise CloseError("VARS.INVALID", str(error), f"Supply vars compatible with {template}.") from error

    def send(self, recipient, template):
        self.notify(recipient, "--template", str(TEMPLATES / template), "--vars", str(self.path))

    def notify(self, recipient, *args):
        """`atm send` to recipient. ATM refuses a self-addressed send, so a closer who is the recipient (a lead closing a task it took over) is not sent its own close."""
        if recipient == self.identity:
            return
        self.run(["atm", "send", recipient, *args])

    def qa_reviewers(self, qa_bead):
        """`metadata.reviewers_round1` of the QA bead as the list qa-template.xml.j2 iterates; anything else is refused before any write."""
        reviewers = (self.read_bead(qa_bead).get("metadata") or {}).get("reviewers_round1")
        if not reviewer_list(reviewers):
            raise CloseError("BEAD.INVALID", f"{qa_bead} metadata.reviewers_round1 must be a non-empty list of reviewer names, got {json.dumps(reviewers)}",
                             f"Add reviewers_round1 (a JSON list of names) to {qa_bead}'s metadata with `bd update {qa_bead} --metadata <full metadata JSON>` "
                             "(--set-metadata stores a string; qa-bead.json.j2 pours the list from the repository's reviewers_round1), then rerun the hook.")
        return reviewers

    def current_pr_for_qa(self, checked):
        """Read the checked fix's live PR before building its QA assignment."""
        metadata = checked.get("metadata") or {}
        number = metadata.get("pr_number")
        if number in (None, ""):
            return None
        argv = ["gh", "pr", "view", str(number), "--json", "number,baseRefName,headRefName,headRefOid"]
        try:
            row = self.json(argv)
        except CloseError as error:
            raise close_fields.CloseFieldsError(
                "ASSIGN_VARS.PR_READ_FAILED", f"{' '.join(argv)} failed: {error}",
                f"Retry `{' '.join(argv)}` and inspect its output; do not use first-close base or commit values.",
            ) from error
        if not isinstance(row, dict):
            raise close_fields.CloseFieldsError(
                "ASSIGN_VARS.PR_READ_FAILED", f"{' '.join(argv)} did not return a PR object",
                f"Retry `{' '.join(argv)}` and inspect its output; do not use first-close base or commit values.",
            )
        return {"number": row.get("number"), "base": row.get("baseRefName"),
                "head": row.get("headRefName"), "head_sha": row.get("headRefOid")}

    def sanity_dispatch(self, context):
        """The sanity assignment vars, rendered and size-checked; built before anything is closed (#557)."""
        context = {**context, "base": context.get("rebased_onto") or context.get("base")}
        base = context.get("base")
        if not isinstance(base, str) or not re.fullmatch(r"[0-9a-f]{40}", base):
            raise CloseError("BASE.PR_UNVERIFIED", f"sanity base {base!r} is not a full 40-hex commit",
                             "Ask the dispatcher to resolve the PR base to a full 40-hex SHA and rerun.")
        template = "dev-sanity-template.xml.j2"
        pr_number = str(context.get("pr_number") or self.values.get("pr_number") or "")
        if not context.get("layer_prs") and pr_number:
            # The checked PR is the one layer sanity-split must read (`--layer-pr`): its body is the dev's evidence (#636).
            context = {**context, "layer_prs": [pr_number]}
        close_context = {name: context[name] for name in SANITY_CLOSE_CONTEXT if name in context}
        if close_fields.is_fix_bead(self.bead(context["checked_bead"])):
            close_context["carry_forward"] = context["checked_bead"]
        # The QA reviewer list is planned on the QA bead (qa-bead.json.j2); the sanity close renders qa-template with it (#821).
        close_context["reviewers_round1"] = self.qa_reviewers(context["qa_bead"])
        close_context.update(phase_lead=self.lead, budget=context.get("budget", "full"))
        dispatch = {**context, "close_context": close_context, "beads_dir": self.beads_dir or self.beads_directory()}
        phase = str(context.get("phase") or "")
        rows = self.json(["atm", "hook", "list", "--phase", phase, "--json"], lead=True) if phase else []
        dispatch["sanity_close_mode"] = sanity_close_mode(rows, phase, self.config["dev_sanity_member"])
        self.render(template, dispatch)
        size = len(json.dumps(dispatch).encode()) + len((TEMPLATES / template).read_bytes())
        if size > ATM_MAX_REQUEST_BYTES:
            largest = sorted(dispatch, key=lambda name: len(json.dumps(dispatch[name]).encode()), reverse=True)[:3]
            named = ", ".join(f"{name} ({len(json.dumps(dispatch[name]).encode())} bytes)" for name in largest)
            raise CloseError("ASSIGNMENT.TOO_LARGE",
                             f"sanity assignment is {size} bytes, over ATM's {ATM_MAX_REQUEST_BYTES}-byte request limit; largest fields: {named}",
                             f"Shorten {largest[0]} in --result, then rerun with the same vars file; nothing was closed.")
        return dispatch

    def assign(self, member, bead, template, *, path=None, due=None, dispatch=None):
        events = self.task_history[bead]
        if due is None:
            due = not events or (is_terminal(events) and self.bead(bead).get("status") == "open")
        if due:
            if template == "dev-sanity-template.xml.j2":
                with tempfile.TemporaryDirectory(prefix="sanity-dispatch-") as directory:
                    dispatch_path = Path(directory) / "completion-vars.json"
                    dispatch_path.write_text(json.dumps(dispatch))
                    self.run(["atm", "task", "assign", member, "--task-id", bead, "--template", str(TEMPLATES / template),
                              "--vars", str(dispatch_path)], lead=True)
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
        head = (getattr(self, "checked_worktree_head", None)
                or self.run(["git", "rev-parse", "HEAD"], git=True).stdout.strip())
        self.values.update(commit=head, generated_at=datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
                           pr_number="", pr_url="", rebased_onto="")
        if self.role == "fix" and self.values["outcome"] == "not_reproducible":
            self.values["branch"] = self.checked_worktree_branch
        self.require_completion_identity(("commit",))
        self.key = [self.task, self.values.get("verdict", ""), head]

    def validate_not_reproducible_checkout(self, metadata):
        """Pin a not-reproducible handoff to the checked bead's selected checkout."""
        branch = metadata.get("branch")
        worktree = metadata.get("worktree")
        if not branch or not worktree:
            raise CloseError("BEAD.INVALID", f"{self.values['checked_bead']}: metadata lacks branch or worktree",
                             f"Run `bd update {self.values['checked_bead']} --metadata '<full metadata JSON with branch and worktree>'`, then retry.")
        recorded_worktree = Path(worktree)
        if not recorded_worktree.is_absolute():
            recorded_worktree = self.root / recorded_worktree
        recorded_worktree = recorded_worktree.resolve()
        if recorded_worktree != self.worktree:
            raise CloseError("CHECKOUT.MISMATCH",
                             f"{self.values['checked_bead']} records worktree {recorded_worktree}, but the selected worktree is {self.worktree}",
                             f"Select the checked bead's worktree or repair its metadata with `bd update {self.values['checked_bead']} --metadata '<full metadata JSON with the correct worktree>'`, then retry.")
        actual_branch = self.run(["git", "branch", "--show-current"], git=True).stdout.strip()
        actual_head = self.run(["git", "rev-parse", "HEAD"], git=True).stdout.strip()
        expected_head = metadata.get("commit")
        if not expected_head or actual_branch != branch or actual_head != expected_head:
            expected = f"{branch}@{expected_head or '(commit not recorded)'}"
            actual = f"{actual_branch or '(detached)'}@{actual_head or '(unknown)'}"
            raise CloseError("CHECKOUT.MISMATCH",
                             f"{self.values['checked_bead']} records {expected}, but the selected worktree is {actual}",
                             f"Check out the recorded branch and commit in {self.worktree}, or repair the checked bead's branch/commit metadata with `bd update {self.values['checked_bead']} --metadata '<full metadata JSON with the correct branch and commit>'`, then retry.")
        self.checked_worktree_branch = actual_branch
        self.checked_worktree_head = actual_head

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
                self.resolve_nr_base(skip_checks=skip)
                assignment = {**values, "branch": self.checked_worktree_branch, "commit": self.checked_worktree_head,
                              "worktree_path": str(self.worktree)}
                assignment_path = Path(directory) / "sanity-vars.json"
                assignment_path.write_text(json.dumps(assignment), encoding="utf-8")
                self.render(template)
                completion.write_text(json.dumps(values), encoding="utf-8")
                self.path = completion
                self.finish_dev(sanity, template, path=assignment_path)
                return
            if not skip:
                self.check_record = self.checks()
                values["rebased_onto"] = self.rebased_onto
            else:
                self.keep_recorded_pr()
                self.require_completion_identity()
                self.resolve_recorded_pr_base(skip_stack=True)
                values["rebased_onto"] = self.rebased_onto
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
        bound = checked_metadata.get("sanity_assignment_id")
        if open_assignment:
            self.verify_sanity_assignment(sanity, sanity_events, bound)
        sha_changed = checked_metadata.get("commit") != assignment_values["commit"]
        assign = (not terminal or self.bead(sanity).get("status") == "open") and (sha_changed or not open_assignment)
        if not assign and sha_changed:
            raise CloseError("REPLAY.CONFLICT", "sanity is closed at a different sha", "Request a new sanity assignment before changing the checked sha.")
        # The assignment is built and checked against ATM's limit before the bead closes: after it, a refused send strands the close.
        dispatch = self.sanity_dispatch(assignment_values) if assign else None
        self.metadata(values["checked_bead"], {"commit": assignment_values["commit"], "close_assignment_id": str(self.assignment_id), "branch": assignment_values["branch"], "pr_number": values.get("pr_number", ""),
                                             "pr_url": values["pr_url"], **({"pr_base": self.pr_base} if self.pr_base else {})})
        reason = f"not reproducible: {values['evidence']}" if values["outcome"] == "not_reproducible" else f"{'fixed' if self.role == 'fix' else 'dev complete'} at {values['commit']}"
        if self.bead(values["checked_bead"]).get("status") == "closed" and self.bead(values["checked_bead"]).get("close_reason") != reason:
            self.update(values["checked_bead"], status="open")
        self.close_bead(values["checked_bead"], reason)
        if assign and not open_assignment:
            self.send(self.lead, template)
        self.assign(self.config["dev_sanity_member"], sanity, "dev-sanity-template.xml.j2", due=assign, dispatch=dispatch)
        actual = self.read_bead(values["checked_bead"])
        history = self.events(sanity)
        assignments = [event for event in history if event.get("event") in ASSIGNMENT_EVENTS]
        previous = [event for event in sanity_events if event.get("event") in ASSIGNMENT_EVENTS]
        new_assignment_required = assign and not open_assignment
        if actual.get("status") != "closed" or actual.get("close_reason") != reason or not assignments or (new_assignment_required and len(assignments) != len(previous) + 1):
            raise CloseError("CLOSE.POSTCONDITION", f"expected {values['checked_bead']} closed with reason {reason!r} and {sanity} assigned; "
                             f"read status {actual.get('status')!r}, reason {actual.get('close_reason')!r} and {len(assignments)} assignment(s)",
                             f"Inspect `bd show {values['checked_bead']} --json` and `atm task events {sanity} --json`, repair the state and retry.")
        if new_assignment_required:
            bound = assignments[-1].get("message_id") or assignments[-1].get("seq")
        bound = self.verify_sanity_assignment(sanity, history, bound)
        self.metadata(values["checked_bead"], {"sanity_assignment_id": bound})
        actual = self.read_bead(values["checked_bead"])
        stored_bound = (actual.get("metadata") or {}).get("sanity_assignment_id")
        if stored_bound is None or str(stored_bound) != bound:
            raise CloseError("CLOSE.POSTCONDITION", f"{values['checked_bead']}: expected sanity_assignment_id {bound!r}, observed {(actual.get('metadata') or {}).get('sanity_assignment_id')!r}",
                             f"Inspect `bd show {values['checked_bead']} --json` and repair the recorded assignment, then retry.")
        self.verify_sanity_assignment(sanity, self.events(sanity), bound)
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
            current_pr = self.current_pr_for_qa(checked)
            qa_values = close_fields.qa_assignment_vars(self.bead(values["qa_bead"]), checked, self.root, current_pr)
            if qa_values["checked_bead"] != checked["id"]:
                raise CloseError("ASSIGN_VARS.CONFLICT", f"{values['qa_bead']} points to {qa_values['checked_bead']}, but sanity checked {checked['id']}",
                                 f"Correct metadata.checked_bead on {values['qa_bead']} to {checked['id']} and retry.")
            if type(qa_values["round"]) is not int or qa_values["round"] < 1:
                raise CloseError("BEAD.INVALID", f"{values['qa_bead']} metadata.round must be a positive integer",
                                 f"Run `bd update {values['qa_bead']} --set-metadata round=<positive integer>` and retry.")
            self.render("qa-template.xml.j2", qa_values)
            if (self.bead(bead).get("status") == "closed"
                    and self.bead(bead).get("close_reason") != f"PASS at {values['commit']}"
                    and not is_terminal(self.task_history[bead])):
                self.update(bead, status="open")
            self.close_bead(bead, f"PASS at {values['commit']}")
            self.close_task(bead, "dev-sanity-complete.md.j2")
            with tempfile.TemporaryDirectory(prefix="qa-dispatch-") as directory:
                qa_path = Path(directory) / "qa-vars.json"
                qa_path.write_text(json.dumps(qa_values), encoding="utf-8")
                self.assign(self.config["qa_member"], values["qa_bead"], "qa-template.xml.j2", path=qa_path)
            return
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
                self.notify(self.config["team_lead_member"], message)
            self.close_task(bead, "dev-sanity-complete.md.j2")
            return "SANITY.ROUND_CAP"
        checked = self.bead(values["checked_bead"])
        template, dispatch = close_fields.reassignment(self.root, values["phase"], checked, self.config)
        sc_render.render(template, dispatch)
        notes = f"sanity FAIL round {count} at {values['commit']}:\n" + "\n".join(
            f"{item['number']}. {item['reason']}" for item in values["undone"])
        self.write_bead(values["checked_bead"], "update", {"append_notes": notes})
        if checked.get("status") != "open":
            self.update(values["checked_bead"], status="open", assignee=checked["assignee"])
        if record.get("status") != "open" or record.get("assignee"):
            self.update(bead, status="open", assignee="")
        events = self.task_history[bead]
        if not is_terminal(events):
            with tempfile.TemporaryDirectory(prefix="dev-reassignment-") as directory:
                path = Path(directory) / "vars.json"
                path.write_text(json.dumps(dispatch))
                self.run(["atm", "task", "assign", checked["assignee"], "--task-id", values["checked_bead"],
                          "--template", str(template), "--vars", str(path)], lead=True)
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
            self.notify(self.lead, message)
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
        global COMMAND_TIMEOUT
        try:
            COMMAND_TIMEOUT = repo_timeout()  # resolved per run, so a configuration change applies without a reload
        except ValueError as error:
            raise CloseError("CONFIG.INVALID", str(error), f"Unset {TIMEOUT_ENV} or set it to a positive integer number of seconds, e.g. {TIMEOUT_ENV}=180.") from error
        values = read_vars(args.vars)
        if args.role in ("dev", "fix"):
            if args.result is None:
                raise CloseError("ARGS.INVALID", "--result is required for dev and fix",
                                 "Pass the fixer's result file with --result <result-file>.")
            values = merge_result(values, read_vars(args.result), args.role)
            values = validate_outcome(values, args.role, args.result)
        validate_shared_vars(values, args.role)
        hook = Hook(args.role, values, args.vars, runner, clock, sleep, sample)
        hook.beads_directory()
        if args.role == "fix" and values.get("outcome") == "not_reproducible":
            checked = hook.read_bead(values["checked_bead"])
            metadata = checked.get("metadata") or {}
            hook.validate_not_reproducible_checkout(metadata)
            hook.verify_not_reproducible_evidence()
        started = clock()
        escalated = hook.dev(args.skip_checks) if args.role in ("dev", "fix") else hook.sanity() if args.role == "sanity" else hook.qa()
        result = {"role": args.role, "bead": hook.task, "commit": values.get("commit", ""), "pr_number": values.get("pr_number", ""),
                  "beads_dir": hook.check_record.get("beads_dir", hook.beads_dir),
                  "github": hook.github, "assigned": hook.assigned, "closed": hook.closed, "elapsed_s": max(0, clock() - started)}
        result.update(ledger_top=hook.check_record.get("ledger_top", hook.ledger_top),
                      pr_base=hook.check_record.get("pr_base", hook.base_evidence))
        if hook.base_difference:
            result["base_difference"] = hook.base_difference
        if hook.github == "unavailable":
            result["github_error"] = hook.github_error
        if escalated:
            result["escalated"] = escalated
        output = {"ok": result}
    except (CloseError, close_fields.CloseFieldsError) as error:
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
