"""Shared process helpers, the phase file `.atm-bd/<phase>.toml` and the plan file.

`<phase>` is `phase-<x>`. The tracked `.atm-bd/phase-<x>.toml` names the
plan file, the phase root and the integration branch:

    plan = "<plans_dir>/phase-<x>.jsonl"
    root = "<prefix>-phase-<x>"
    integration_branch = "integrate/phase-<x>"

The plan file holds one JSON object per sprint, `{"sprint": "<name>"}` with an
optional `"depends_on": ["<sprint name>", ...]` (hard dependencies only). It
locks the sprint set. The sprint container id is `<prefix>-<sprint>` (prefix
from the root id); its poured group is `<container>.group-{dev,sanity,qa}`.
"""
from __future__ import annotations
import json
import re
import subprocess
import tomllib
from pathlib import Path
from typing import Any

import repo_config

PHASE = re.compile(r"[a-z0-9]+")
SPRINT_NAME = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*")
# The sprint-group formula's attach ref (atm-bd-orchestration/formulas/sprint-group.relations.json).
GROUP_REF = "group"
PHASE_DIR = Path(".atm-bd")
PHASE_KEYS = ("plan", "root", "integration_branch")
PLAN_KEYS = frozenset({"sprint", "depends_on"})


def run_json(repo: Path, *args: str) -> Any:
    result = subprocess.run(list(args), cwd=repo, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"{' '.join(args)} failed: {result.stderr.strip() or result.stdout.strip()}")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"{' '.join(args)} returned invalid JSON: {exc}") from exc


def phase_id(root: str, record: dict) -> str:
    phase = (record.get("metadata") or {}).get("phase")
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


def phase_config_path(repo: Path, phase: str) -> Path:
    if not PHASE.fullmatch(phase):
        raise RuntimeError(f"invalid phase: {phase}")
    return Path(repo) / PHASE_DIR / f"phase-{phase}.toml"


def load_phase_config(repo: Path, phase: str) -> dict[str, str]:
    """The tracked `.atm-bd/phase-<phase>.toml`: exactly plan, root and integration_branch, all nonempty strings."""
    path = phase_config_path(repo, phase)
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError as exc:
        raise RuntimeError(f"{path} does not exist; the planner commits it with the plan") from exc
    except (OSError, tomllib.TOMLDecodeError) as exc:
        raise RuntimeError(f"{path}: {exc}") from exc
    if set(data) != set(PHASE_KEYS) or not all(isinstance(data[k], str) and data[k] for k in PHASE_KEYS):
        raise RuntimeError(f"{path} must hold exactly {', '.join(PHASE_KEYS)}, each a nonempty string")
    if phase_of_root(data["root"]) != phase:
        raise RuntimeError(f"{path}: root {data['root']} is not a root of phase {phase}")
    return dict(data)


def phase_path(repo: Path, phase: str) -> Path:
    """`<plans_dir>/phase-<phase>.jsonl`; plans_dir comes from the repository configuration."""
    if not PHASE.fullmatch(phase):
        raise RuntimeError(f"invalid phase: {phase}")
    return Path(repo) / repo_config.load(repo)["plans_dir"] / f"phase-{phase}.jsonl"


def bead_prefix(root: str | None, start: Path) -> str:
    """The bead id prefix: taken from the phase root id (`<prefix>-phase-<p>`) when one is given,
    else `bead_prefix` from the repository configuration of the repository holding `start`."""
    if root:
        match = re.fullmatch(r"(.+)-phase-[a-z0-9]+", root)
        if not match:
            raise RuntimeError(f"root bead id {root!r} is not <prefix>-phase-<phase>")
        return match.group(1)
    return str(repo_config.load(repo_config.repo_root(start))["bead_prefix"])


def sprint_bead_id(prefix: str, sprint: str) -> str:
    return f"{prefix}-{sprint}"


def group_id(container: str, step: str) -> str:
    """A poured sprint-group bead: `<container>.group-<step>` (bead_formulas.attach_id)."""
    return f"{container}.{GROUP_REF}-{step}"


def parse_plan(text: str, where: str = "plan") -> tuple[list[dict], list[str]]:
    """The plan file's rows and every schema problem in it ("<where>:<line>: <problem>")."""
    rows: list[dict] = []
    problems: list[str] = []
    seen: set[str] = set()
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        at = f"{where}:{number}"
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            problems.append(f"{at}: not JSON: {exc}")
            continue
        if not isinstance(value, dict) or "sprint" not in value or set(value) - PLAN_KEYS:
            problems.append(at + ': each line is {"sprint": "<name>"} with an optional "depends_on" list, nothing else')
            continue
        sprint, depends_on = value["sprint"], value.get("depends_on", [])
        if not isinstance(sprint, str) or not SPRINT_NAME.fullmatch(sprint):
            problems.append(f"{at}: sprint {sprint!r} is not a lowercase a-z0-9- name")
            continue
        if not isinstance(depends_on, list) or not all(isinstance(d, str) and d for d in depends_on):
            problems.append(f"{at}: {sprint}: depends_on must be a list of sprint names")
            continue
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


def normalized(rows: list[dict], root: str) -> dict:
    """The in-memory view consumers read: per sprint its container, poured dev and sanity ids and the
    sanity ids of its predecessors."""
    prefix = bead_prefix(root, Path("."))
    sanity = {r["sprint"]: group_id(sprint_bead_id(prefix, r["sprint"]), "sanity") for r in rows}
    out = []
    for row in rows:
        container = sprint_bead_id(prefix, row["sprint"])
        out.append({"sprint_bead_id": container, "dev_bead_id": group_id(container, "dev"),
                    "sanity_bead_id": sanity[row["sprint"]],
                    "depends_on_sanity_bead_ids": [sanity[d] for d in row["depends_on"]]})
    result = {"root_bead_id": root, "sprints": out}
    validate_index(result)
    return result


def load_phase_plan(path: Path, root: str | None = None) -> dict:
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


def index_bead_pairs(index: dict) -> dict[str, str]:
    """dev bead id -> sanity bead id, validated."""
    pairs: dict[str, str] = {}
    rows = index.get("sprints")
    if not isinstance(rows, list):
        raise RuntimeError("sprint index must contain a sprints array")
    for row in rows:
        allowed = {"sprint_bead_id", "dev_bead_id", "sanity_bead_id", "depends_on_sanity_bead_ids"}
        if not isinstance(row, dict) or not {"dev_bead_id", "sanity_bead_id"} <= set(row) or set(row) - allowed:
            raise RuntimeError("each normalized sprint item must contain dev_bead_id, sanity_bead_id, and optional sprint_bead_id, depends_on_sanity_bead_ids")
        dev, gate = row.get("dev_bead_id"), row.get("sanity_bead_id")
        if not isinstance(dev, str) or not dev:
            raise RuntimeError("dev_bead_id must be a nonempty bead ID")
        if not isinstance(gate, str) or not gate or gate == dev:
            raise RuntimeError(f"{dev}: missing or invalid sanity_bead_id")
        if dev in pairs or gate in pairs.values():
            raise RuntimeError(f"{dev}: duplicate dev or sanity bead in sprint index")
        deps = row.get("depends_on_sanity_bead_ids", [])
        if not isinstance(deps, list) or not all(isinstance(dep, str) and dep for dep in deps):
            raise RuntimeError(f"{dev}: depends_on_sanity_bead_ids must be a list of bead IDs")
        if len(set(deps)) != len(deps) or gate in deps:
            raise RuntimeError(f"{dev}: invalid duplicate or self sanity dependency")
        pairs[dev] = gate
    if set(pairs) & set(pairs.values()):
        raise RuntimeError("a sanity bead cannot also be a sprint dev bead")
    known_sanity = set(pairs.values())
    for row in rows:
        for dep in row.get("depends_on_sanity_bead_ids", []):
            if dep not in known_sanity:
                raise RuntimeError(f"{row['dev_bead_id']}: dependency {dep} is not a listed sprint sanity bead")
    return pairs


def sprint_beads(index: dict) -> dict[str, str]:
    """dev bead id -> its sprint container id (the dev id itself for an index without containers)."""
    return {row["dev_bead_id"]: row.get("sprint_bead_id", row["dev_bead_id"]) for row in index["sprints"]}


REQUIRED_KEYS = frozenset({"root_bead_id", "sprints"})
# Phase facts that are not in beads (user ruling 2026-09-26); membership rows stay exactly as before.
OPTIONAL_KEYS = frozenset({"integration_branch", "review_artifacts", "policy"})
POLICY_KEYS = frozenset({"human_gates", "waivers"})
WAIVABLE_CHECKS = frozenset({"reopened_after_pass", "started_before_blocker", "pass_without_qa"})


def validate_index(index: Any) -> None:
    """Enforce the document contract: membership rows plus declared phase facts, nothing else."""
    if not isinstance(index, dict) or not REQUIRED_KEYS <= set(index):
        raise RuntimeError("phase index must contain root_bead_id and sprints")
    unknown = set(index) - REQUIRED_KEYS - OPTIONAL_KEYS
    if unknown:
        raise RuntimeError("phase index may contain only root_bead_id and sprints plus the declared phase facts "
                           f"({', '.join(sorted(OPTIONAL_KEYS))}); undeclared keys: {', '.join(sorted(unknown))}")
    branch = index.get("integration_branch")
    if branch is not None and (not isinstance(branch, str) or not branch):
        raise RuntimeError("integration_branch must be a nonempty branch name")
    artifacts = index.get("review_artifacts")
    if artifacts is not None and (not isinstance(artifacts, list) or not all(isinstance(a, str) and a for a in artifacts)):
        raise RuntimeError("review_artifacts must be a list of repository paths")
    policy = index.get("policy")
    if policy is not None:
        if not isinstance(policy, dict) or set(policy) - POLICY_KEYS:
            raise RuntimeError(f"policy may only contain {', '.join(sorted(POLICY_KEYS))}")
        gates = policy.get("human_gates")
        if gates is not None and (not isinstance(gates, list) or not all(isinstance(g, str) and g for g in gates)):
            raise RuntimeError("policy.human_gates must be a list of gate bead IDs")
        waivers = policy.get("waivers")
        if waivers is not None:
            if not isinstance(waivers, list):
                raise RuntimeError("policy.waivers must be a list of {bead, check, reason} objects")
            for w in waivers:
                if not isinstance(w, dict) or set(w) != {"bead", "check", "reason"} or not all(isinstance(w[k], str) and w[k] for k in w):
                    raise RuntimeError("each policy.waivers entry has exactly bead, check and reason, all nonempty strings")
                if w["check"] not in WAIVABLE_CHECKS:
                    raise RuntimeError(f"policy.waivers check must be one of {', '.join(sorted(WAIVABLE_CHECKS))}")
    root = index["root_bead_id"]
    if not isinstance(root, str) or not root:
        raise RuntimeError("root_bead_id must be a nonempty bead ID")
    pairs = index_bead_pairs(index)
    if root in pairs or root in pairs.values():
        raise RuntimeError("phase root cannot also be a dev or sanity bead")
