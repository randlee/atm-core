# pyright: strict
"""Shared process helpers, the phase file `.atm-bd/<phase>.toml` and the plan file.

`<phase>` is `phase-<x>`. The tracked `.atm-bd/phase-<x>.toml` names the
plan file, the phase root and the integration branch:

    plan = "<plans_dir>/phase-<x>.jsonl"
    root = "<prefix>-phase-<x>"
    integration_branch = "integrate/phase-<x>"
    lead = "appointed-phase-lead"

The required `templates` table names the installed assignment template for
each of dev, fix, sanity, qa, plan_review and review.

The plan file holds one JSON object per sprint, `{"sprint": "<name>"}` with an
optional `"depends_on": ["<sprint name>", ...]` (hard dependencies only). It
locks the sprint set. The sprint container id is `<prefix>-<sprint>` (prefix
from the root id); its poured group is `<container>.group-{dev,sanity,qa}`.
"""
from __future__ import annotations
import json
import os
import re
import subprocess
import tomllib
from pathlib import Path
from typing import NotRequired, TypeAlias, TypeGuard, TypedDict

import repo_config
from operation_models import BeadRecord

PHASE = re.compile(r"[a-z0-9]+")
SPRINT_NAME = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
# The sprint-group formula's attach ref (atm-bd-orchestration/formulas/sprint-group.relations.json).
GROUP_REF = "group"
PHASE_DIR = Path(".atm-bd")
PHASE_KEYS = ("plan", "root", "integration_branch")
TEMPLATE_DIR = Path(".claude/skills/atm-bd-orchestration/templates")
DEFAULT_TEMPLATES = {
    "dev": str(TEMPLATE_DIR / "dev-template.xml.j2"),
    "fix": str(TEMPLATE_DIR / "fix-assignment.xml.j2"),
    "sanity": str(TEMPLATE_DIR / "dev-sanity-template.xml.j2"),
    "qa": str(TEMPLATE_DIR / "qa-template.xml.j2"),
    "plan_review": str(TEMPLATE_DIR / "plan-review-template.xml.j2"),
    "review": str(TEMPLATE_DIR / "review-template.xml.j2"),
}
PLAN_KEYS = frozenset({"sprint", "depends_on"})

JsonValue: TypeAlias = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]


class PlanRow(TypedDict):
    sprint: str
    depends_on: list[str]


class PhaseTemplates(TypedDict):
    dev: str
    fix: str
    sanity: str
    qa: str
    plan_review: str
    review: str
    dev_fix: NotRequired[str]


class PhaseConfig(TypedDict):
    plan: str
    root: str
    integration_branch: str
    lead: str
    templates: PhaseTemplates
    code_checkout: NotRequired[str]


class IndexSprint(TypedDict):
    sprint_bead_id: str
    dev_bead_id: str
    sanity_bead_id: str
    depends_on_sanity_bead_ids: list[str]


class NormalizedIndex(TypedDict):
    root_bead_id: str
    sprints: list[IndexSprint]


def is_object_list(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def is_object_mapping(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def is_json_value(value: object) -> TypeGuard[JsonValue]:
    if value is None or isinstance(value, (bool, int, float, str)):
        return True
    if is_object_list(value):
        return all(is_json_value(item) for item in value)
    if is_object_mapping(value):
        for key, item in value.items():
            if not isinstance(key, str) or not is_json_value(item):
                return False
        return True
    return False


def is_json_object(value: object) -> TypeGuard[JsonObject]:
    if not is_object_mapping(value):
        return False
    for key, item in value.items():
        if not isinstance(key, str) or not is_json_value(item):
            return False
    return True


def string_list(value: object) -> TypeGuard[list[str]]:
    return is_object_list(value) and all(isinstance(item, str) for item in value)


def run_json(repo: Path, *args: str) -> JsonValue:
    result = subprocess.run(list(args), cwd=repo, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"{' '.join(args)} failed: {result.stderr.strip() or result.stdout.strip()}")
    try:
        value: object = json.loads(result.stdout)
        if not is_json_value(value):
            raise RuntimeError(f"{' '.join(args)} returned a non-JSON value")
        return value
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"{' '.join(args)} returned invalid JSON: {exc}") from exc


def phase_id(root: str, record: BeadRecord) -> str:
    metadata = record.get("metadata")
    phase = metadata.get("phase") if metadata else None
    if not phase:
        match = re.search(r"-phase-([a-z0-9]+)$", root)
        phase = match.group(1) if match else None
    if not isinstance(phase, str) or not PHASE.fullmatch(phase):
        raise RuntimeError(f"root bead {root} has no valid phase")
    return phase


def phase_of_root(root: str) -> str:
    match = re.fullmatch(r".+-phase-([a-z0-9]+)", root)
    if not match:
        raise RuntimeError(f"root bead id {root!r} is not <prefix>-phase-<phase>")
    return match.group(1)


def required_config_string(values: dict[str, object], key: str) -> str:
    value = values[key]
    if not isinstance(value, str):
        raise RuntimeError(f"phase config {key} must be a string")
    return value


def phase_config_path(repo: Path, phase: str) -> Path:
    if not PHASE.fullmatch(phase):
        raise RuntimeError(f"invalid phase: {phase}")
    return Path(repo) / PHASE_DIR / f"phase-{phase}.toml"


def validate_phase_config(repo: Path, phase: str, raw_data: object) -> PhaseConfig:
    """Validate the complete tracked phase settings."""
    path = phase_config_path(repo, phase)
    if not is_object_mapping(raw_data):
        raise RuntimeError(f"{path}: expected a table at the top level")
    data: dict[str, object] = {}
    for key, value in raw_data.items():
        if not isinstance(key, str):
            raise RuntimeError(f"{path}: expected string keys in the top-level table")
        data[key] = value
    problems: list[str] = []
    for key in PHASE_KEYS:
        value = data.get(key)
        if not isinstance(value, str) or not value.strip():
            problems.append(f"{key}: required nonempty string")
    unknown = set(data) - set(PHASE_KEYS) - {"lead", "templates", "code_checkout"}
    if unknown:
        problems.append(f"unknown key(s): {', '.join(sorted(unknown))}")
    code_checkout = data.get("code_checkout")
    if "code_checkout" in data and (
        not isinstance(code_checkout, str)
        or not code_checkout.strip()
        or Path(code_checkout).is_absolute()
    ):
        problems.append("code_checkout: optional repository-relative path to the code repository")
    lead = data.get("lead")
    if not isinstance(lead, str) or not lead.strip():
        problems.append("lead: required nonempty string")
    templates = data.get("templates")
    if templates is None:
        problems.append("templates: required table with keys " + ", ".join(DEFAULT_TEMPLATES))
    if templates is not None:
        if not is_object_mapping(templates):
            problems.append("templates: expected a table with keys " + ", ".join(DEFAULT_TEMPLATES))
            templates = None
    template_values: dict[str, object] = {}
    if is_object_mapping(templates):
        if any(not isinstance(key, str) for key in templates):
            problems.append("templates: expected string keys")
            templates = None
    if is_object_mapping(templates):
        template_values = {key: value for key, value in templates.items() if isinstance(key, str)}
        extra = set(template_values) - set(DEFAULT_TEMPLATES) - {"dev_fix"}
        if extra:
            problems.append(f"templates: unknown key(s): {', '.join(sorted(extra))}")
        if "dev_fix" in template_values and not isinstance(template_values["dev_fix"], str):
            problems.append("templates.dev_fix: expected a repository-relative template path")
        for key in DEFAULT_TEMPLATES:
            value = template_values.get(key)
            if not isinstance(value, str) or not value.strip():
                problems.append(f"templates.{key}: required nonempty repository-relative path")
                continue
            try:
                relative = Path(value)
                target = Path(repo) / relative
                if (value.endswith("/") or relative.is_absolute() or ".." in relative.parts
                        or not relative.is_relative_to(TEMPLATE_DIR)
                        or not target.resolve().is_relative_to((Path(repo) / TEMPLATE_DIR).resolve())):
                    problems.append(f"templates.{key}: {value!r} must be a repository-relative path inside {TEMPLATE_DIR}")
                elif not os.path.isfile(os.path.join(repo, value)):
                    problems.append(f"templates.{key}: {value!r} does not name an existing template file")
            except (OSError, ValueError, RuntimeError) as exc:
                problems.append(f"templates.{key}: {value!r} cannot be read: {exc}")
    if problems:
        raise RuntimeError(f"{path}: " + "; ".join(problems))
    root = data.get("root")
    if not isinstance(root, str):
        raise RuntimeError(f"{path}: root must be a nonempty phase root")
    if not re.fullmatch(rf".+-phase-{re.escape(phase)}", root):
        raise RuntimeError(f"{path}: root {root} is not a root of phase {phase}")
    typed_templates: PhaseTemplates = {
        "dev": required_config_string(template_values, "dev"),
        "fix": required_config_string(template_values, "fix"),
        "sanity": required_config_string(template_values, "sanity"),
        "qa": required_config_string(template_values, "qa"),
        "plan_review": required_config_string(template_values, "plan_review"),
        "review": required_config_string(template_values, "review"),
    }
    if "dev_fix" in template_values:
        typed_templates["dev_fix"] = required_config_string(template_values, "dev_fix")
    result: PhaseConfig = {
        "plan": required_config_string(data, "plan"),
        "root": root,
        "integration_branch": required_config_string(data, "integration_branch"),
        "lead": required_config_string(data, "lead"),
        "templates": typed_templates,
    }
    if "code_checkout" in data:
        result["code_checkout"] = required_config_string(data, "code_checkout")
    return result


def load_phase_config(repo: Path, phase: str) -> PhaseConfig:
    """Read and validate the complete phase settings."""
    path = phase_config_path(repo, phase)
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"{path} does not exist; the planner commits it with the plan") from exc
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise RuntimeError(f"{path}: {exc}") from exc
    return validate_phase_config(repo, phase, data)


def phase_path(repo: Path, phase: str) -> Path:
    """`<plans_dir>/phase-<phase>.jsonl`; plans_dir comes from the repository configuration."""
    if not PHASE.fullmatch(phase):
        raise RuntimeError(f"invalid phase: {phase}")
    plans_dir = repo_config.load(repo)["plans_dir"]
    return Path(repo) / plans_dir / f"phase-{phase}.jsonl"


def bead_prefix(root: str | None, start: Path) -> str:
    """The bead id prefix: taken from the phase root id (`<prefix>-phase-<p>`) when one is given,
    else `bead_prefix` from the repository configuration of the repository holding `start`."""
    if root:
        match = re.fullmatch(r"(.+)-phase-[a-z0-9]+", root)
        if not match:
            raise RuntimeError(f"root bead id {root!r} is not <prefix>-phase-<phase>")
        return match.group(1)
    return repo_config.load(repo_config.repo_root(start))["bead_prefix"]


def sprint_bead_id(prefix: str, sprint: str) -> str:
    return f"{prefix}-{sprint}"


def group_id(container: str, step: str) -> str:
    """A poured sprint-group bead: `<container>.group-<step>` (bead_formulas.attach_id)."""
    return f"{container}.{GROUP_REF}-{step}"


REQUIRED_KEYS = frozenset({"root_bead_id", "sprints"})
# Phase facts that are not in beads (user ruling 2026-09-26); membership rows stay exactly as before.
OPTIONAL_KEYS = frozenset({"integration_branch", "review_artifacts", "policy"})
POLICY_KEYS = frozenset({"human_gates", "waivers"})
WAIVABLE_CHECKS = frozenset({"reopened_after_pass", "started_before_blocker", "pass_without_qa"})


def parse_plan(text: str, where: str = "plan") -> tuple[list[PlanRow], list[str]]:
    """The plan file's rows and every schema problem in it ("<where>:<line>: <problem>")."""
    rows: list[PlanRow] = []
    problems: list[str] = []
    seen: set[str] = set()
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        at = f"{where}:{number}"
        try:
            value: object = json.loads(line)
        except json.JSONDecodeError as exc:
            problems.append(f"{at}: not JSON: {exc}")
            continue
        if not is_json_object(value) or "sprint" not in value or set(value) - PLAN_KEYS:
            problems.append(at + ': each line is {"sprint": "<name>"} with an optional "depends_on" list, nothing else')
            continue
        sprint = value.get("sprint")
        if not isinstance(sprint, str) or not SPRINT_NAME.fullmatch(sprint):
            problems.append(f"{at}: sprint {sprint!r} is not a lowercase a-z0-9- name")
            continue
        depends_value = value.get("depends_on", [])
        if not string_list(depends_value) or not all(dep for dep in depends_value):
            problems.append(f"{at}: {sprint}: depends_on must be a list of sprint names")
            continue
        depends_on = depends_value
        if sprint in seen:
            problems.append(f"{at}: sprint {sprint} is listed twice")
            continue
        if sprint in depends_on or len(set(depends_on)) != len(depends_on):
            problems.append(f"{at}: {sprint}: depends_on has a duplicate or names the sprint itself")
            depends_on = [d for d in dict.fromkeys(depends_on) if d != sprint]
        seen.add(sprint)
        rows.append({"sprint": sprint, "depends_on": depends_on})
    if not rows and not problems:
        problems.append(f"{where}: the plan lists no sprints")
    for row in rows:
        unknown = [d for d in row["depends_on"] if d not in seen]
        if unknown:
            problems.append(f"{where}: {row['sprint']} depends_on unknown sprint(s): {', '.join(unknown)}")
            row["depends_on"] = [d for d in row["depends_on"] if d in seen]
    return rows, problems


def normalized(rows: list[PlanRow], root: str) -> NormalizedIndex:
    """The in-memory view consumers read: per sprint its container, poured dev and sanity ids and the
    sanity ids of its predecessors."""
    prefix = bead_prefix(root, Path("."))
    sanity = {r["sprint"]: group_id(sprint_bead_id(prefix, r["sprint"]), "sanity") for r in rows}
    out: list[IndexSprint] = []
    for row in rows:
        container = sprint_bead_id(prefix, row["sprint"])
        out.append({"sprint_bead_id": container, "dev_bead_id": group_id(container, "dev"),
                    "sanity_bead_id": sanity[row["sprint"]],
                    "depends_on_sanity_bead_ids": [sanity[d] for d in row["depends_on"]]})
    result: NormalizedIndex = {"root_bead_id": root, "sprints": out}
    validate_index(result)
    return result


def load_phase_plan(path: Path, root: str | None = None) -> NormalizedIndex:
    """Parse a plan file (`<plans_dir>/phase-<x>.jsonl`) and return the normalized view; any schema problem raises."""
    path = Path(path)
    rows, problems = parse_plan(path.read_text(encoding="utf-8"), str(path))
    if problems:
        raise RuntimeError("; ".join(problems))
    if root is None:
        match = re.fullmatch(r"phase-([a-z0-9]+)\.jsonl", path.name)
        if not match:
            raise RuntimeError(f"{path}: a plan file is named phase-<phase>.jsonl; pass the root")
        root = f"{bead_prefix(None, path.parent)}-phase-{match.group(1)}"
    return normalized(rows, root)


def index_bead_pairs(raw_index: object) -> dict[str, str]:
    """dev bead id -> sanity bead id, validated."""
    if not is_json_object(raw_index):
        raise RuntimeError("phase index must be a JSON object")
    index = raw_index
    pairs: dict[str, str] = {}
    rows = index.get("sprints")
    if not is_object_list(rows):
        raise RuntimeError("sprint index must contain a sprints array")
    parsed_rows: list[JsonObject] = []
    for row in rows:
        allowed = {"sprint_bead_id", "dev_bead_id", "sanity_bead_id", "depends_on_sanity_bead_ids"}
        if not is_json_object(row) or not {"dev_bead_id", "sanity_bead_id"} <= set(row) or set(row) - allowed:
            raise RuntimeError("each normalized sprint item must contain dev_bead_id, sanity_bead_id, and optional sprint_bead_id, depends_on_sanity_bead_ids")
        parsed_rows.append(row)
        dev, gate = row.get("dev_bead_id"), row.get("sanity_bead_id")
        if not isinstance(dev, str) or not dev:
            raise RuntimeError("dev_bead_id must be a nonempty bead ID")
        if not isinstance(gate, str) or not gate or gate == dev:
            raise RuntimeError(f"{dev}: missing or invalid sanity_bead_id")
        if dev in pairs or gate in pairs.values():
            raise RuntimeError(f"{dev}: duplicate dev or sanity bead in sprint index")
        deps_value = row.get("depends_on_sanity_bead_ids", [])
        if not string_list(deps_value) or not all(dep for dep in deps_value):
            raise RuntimeError(f"{dev}: depends_on_sanity_bead_ids must be a list of bead IDs")
        deps = deps_value
        if len(set(deps)) != len(deps) or gate in deps:
            raise RuntimeError(f"{dev}: invalid duplicate or self sanity dependency")
        pairs[dev] = gate
    if set(pairs) & set(pairs.values()):
        raise RuntimeError("a sanity bead cannot also be a sprint dev bead")
    known_sanity = set(pairs.values())
    for row in parsed_rows:
        deps_value = row.get("depends_on_sanity_bead_ids", [])
        if not string_list(deps_value):
            raise RuntimeError(f"{row['dev_bead_id']}: depends_on_sanity_bead_ids must be a list of bead IDs")
        for dep in deps_value:
            if dep not in known_sanity:
                raise RuntimeError(f"{row['dev_bead_id']}: dependency {dep} is not a listed sprint sanity bead")
    return pairs


def sprint_beads(raw_index: object) -> dict[str, str]:
    """dev bead id -> its sprint container id (the dev id itself for an index without containers)."""
    if not is_json_object(raw_index):
        raise RuntimeError("phase index must be a JSON object")
    rows = raw_index.get("sprints")
    if not is_object_list(rows):
        raise RuntimeError("sprint index must contain a sprints array")
    result: dict[str, str] = {}
    for raw_row in rows:
        if not is_json_object(raw_row) or "dev_bead_id" not in raw_row:
            raise RuntimeError("each normalized sprint item must contain dev_bead_id")
        dev = raw_row.get("dev_bead_id")
        if not isinstance(dev, str):
            raise RuntimeError("dev_bead_id must be a nonempty bead ID")
        container = raw_row.get("sprint_bead_id", dev)
        if not isinstance(container, str):
            raise RuntimeError(f"{dev}: sprint_bead_id must be a string")
        result[dev] = container
    return result


def validate_index(raw_index: object) -> None:
    """Enforce the document contract: membership rows plus declared phase facts, nothing else."""
    if not is_json_object(raw_index) or not REQUIRED_KEYS <= set(raw_index):
        raise RuntimeError("phase index must contain root_bead_id and sprints")
    index = raw_index
    unknown = set(index) - REQUIRED_KEYS - OPTIONAL_KEYS
    if unknown:
        raise RuntimeError("phase index may contain only root_bead_id and sprints plus the declared phase facts "
                           f"({', '.join(sorted(OPTIONAL_KEYS))}); undeclared keys: {', '.join(sorted(unknown))}")
    branch = index.get("integration_branch")
    if branch is not None and (not isinstance(branch, str) or not branch):
        raise RuntimeError("integration_branch must be a nonempty branch name")
    artifacts = index.get("review_artifacts")
    if artifacts is not None and (not string_list(artifacts) or not all(artifact for artifact in artifacts)):
        raise RuntimeError("review_artifacts must be a list of repository paths")
    policy = index.get("policy")
    if policy is not None:
        if not is_json_object(policy) or set(policy) - POLICY_KEYS:
            raise RuntimeError(f"policy may only contain {', '.join(sorted(POLICY_KEYS))}")
        gates = policy.get("human_gates")
        if gates is not None and (not string_list(gates) or not all(gate for gate in gates)):
            raise RuntimeError("policy.human_gates must be a list of gate bead IDs")
        waivers = policy.get("waivers")
        if waivers is not None:
            if not is_object_list(waivers):
                raise RuntimeError("policy.waivers must be a list of {bead, check, reason} objects")
            for w in waivers:
                if not is_json_object(w) or set(w) != {"bead", "check", "reason"}:
                    raise RuntimeError("each policy.waivers entry has exactly bead, check and reason, all nonempty strings")
                waiver_bead = w.get("bead")
                waiver_check = w.get("check")
                waiver_reason = w.get("reason")
                if not all(isinstance(value, str) and value for value in (waiver_bead, waiver_check, waiver_reason)):
                    raise RuntimeError("each policy.waivers entry has exactly bead, check and reason, all nonempty strings")
                if not isinstance(waiver_check, str) or waiver_check not in WAIVABLE_CHECKS:
                    raise RuntimeError(f"policy.waivers check must be one of {', '.join(sorted(WAIVABLE_CHECKS))}")
    root = index["root_bead_id"]
    if not isinstance(root, str) or not root:
        raise RuntimeError("root_bead_id must be a nonempty bead ID")
    pairs = index_bead_pairs(index)
    if root in pairs or root in pairs.values():
        raise RuntimeError("phase root cannot also be a dev or sanity bead")
