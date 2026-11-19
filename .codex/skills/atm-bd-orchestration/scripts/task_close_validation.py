"""Shared validation for task-close events and command results."""

import json
from pathlib import Path
import re
import tomllib

import repo_config
import sc_render

import yaml

TEMPLATES = Path(__file__).resolve().parents[1] / "templates"
ASSIGNMENT_EVENTS = frozenset(("assigned", "reassigned", "reopened"))


class CloseError(RuntimeError):
    def __init__(self, code, message, fix):
        super().__init__(message)
        self.code, self.fix = code, fix


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
            round_number = sum(event.get("event") in ASSIGNMENT_EVENTS for event in events[:index + 1])
            return assignment, assignment_id, round_number
    raise CloseError("REPLAY.CONFLICT", f"{task} has no assignment event", "Assign the tracked task before closing it.")


def assignment_id_for_round(events, task, round_number):
    """Return the assignment recorded for a sanity iteration."""
    for index, assignment in enumerate(events):
        if assignment.get("event") in ASSIGNMENT_EVENTS:
            assignment_round = sum(event.get("event") in ASSIGNMENT_EVENTS for event in events[:index + 1])
            if assignment_round == round_number:
                return assignment.get("message_id") or assignment["seq"]
    return None


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


def header(path):
    return yaml.safe_load(path.read_text(encoding="utf-8").split("---", 2)[1])

def reviewer_list(value):
    return isinstance(value, list) and bool(value) and all(isinstance(item, str) and item.strip() for item in value)

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
    if role == "sanity" and not reviewer_list(values["reviewers_round1"]):
        raise CloseError("VARS.INVALID", f"reviewers_round1 must be a non-empty list of reviewer names, got {json.dumps(values['reviewers_round1'])}",
                         "Take reviewers_round1 from the QA bead's metadata (close-vars.py <bead> --out <vars.json> does); never an empty string.")
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


SANITY_CLOSE_HOOK_MODULE = "sanity_task_close_hook.py"
RESULT_OUTCOMES = {"dev": ("fixed",), "fix": ("fixed", "not_reproducible")}
OUTCOME_ALIASES = {"completed": "fixed"}


def sanity_close_mode(rows, phase, member):
    """`daemon` when `atm hook list --phase <phase> --json` shows an active task.close registration of the sanity
    close hook that enables `member` (SANITY-CLOSE-HOOK.md); otherwise `legacy`."""
    for row in rows if isinstance(rows, list) else []:
        if not isinstance(row, dict):
            continue
        custom = row.get("custom")
        agents = custom.get("sanity_agents") if isinstance(custom, dict) else None
        if (row.get("phase") == phase and row.get("hook") == "task.close" and row.get("active") is True
                and Path(str(row.get("module_path") or "")).name == SANITY_CLOSE_HOOK_MODULE
                and isinstance(agents, list) and member in agents):
            return "daemon"
    return "legacy"

def bd_value(item):
    """A metadata value as bd stores it: lists and objects as their JSON text (readers decode them, e.g.
    sanity_round_key) and integer text as the integer (a vars file carries pr_number as "351")."""
    if isinstance(item, (list, dict)):
        return json.dumps(item, separators=(",", ":"))
    if isinstance(item, str) and item.isdigit() and str(int(item)) == item:
        return int(item)
    return item

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
    result = {key: value for key, value in result.items() if key != "stack_view"}
    fields = set(header(TEMPLATES / "task-close-vars.j2")["result_variables_by_role"][role])
    unknown = result.keys() - fields
    if unknown:
        raise CloseError("VARS.UNKNOWN", ", ".join(sorted(unknown)), f"Remove fields that are not {role} result fields from --result.")
    overlap = values.keys() & result.keys()
    if overlap:
        raise CloseError("VARS.INVALID", ", ".join(sorted(overlap)),
                         "Pass each completion field once: remove it from the lead's dispatch file or from --result.")
    return {**values, **result}

def validate_outcome(values, role, path):
    """Map an aliased result outcome, and refuse a bad one before the template's undefined-filter guard reports it as an unknown filter."""
    allowed = RESULT_OUTCOMES[role]
    if "outcome" not in values:
        return values
    values = {**values, "outcome": OUTCOME_ALIASES.get(values["outcome"], values["outcome"])}
    if values["outcome"] not in allowed:
        raise CloseError("VARS.INVALID", f"{role} outcome must be {' or '.join(allowed)}, got {json.dumps(values['outcome'])}",
                         f"Set outcome to {' or '.join(allowed)} in {path}, then rerun the hook.")
    return values

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
