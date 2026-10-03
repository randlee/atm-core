"""Shared process and canonical phase-plan helpers.

The persisted phase plan is JSONL.  Each nonblank line is exactly
``[sprint_name, sanity_bead_id, depends_on_sprint_names]``.  Sprint dev IDs
are deliberately derived from the sprint name so a second copy cannot drift.
"""
from __future__ import annotations
import json
import re
import subprocess
from pathlib import Path
from typing import Any

import repo_config


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
    if not isinstance(phase, str) or not re.fullmatch(r"[a-z0-9]+", phase):
        raise RuntimeError(f"root bead {root} has no valid phase")
    return phase


def phase_path(repo: Path, phase: str) -> Path:
    """`<plans_dir>/phase-<phase>/sprints.jsonl`; plans_dir comes from the repository configuration."""
    if not re.fullmatch(r"[a-z0-9]+", phase):
        raise RuntimeError(f"invalid phase: {phase}")
    return repo / repo_config.load(repo)["plans_dir"] / f"phase-{phase}" / "sprints.jsonl"


def _phase_from_path(path: Path) -> str:
    match = re.fullmatch(r"phase-([a-z0-9]+)", path.parent.name)
    if not match:
        raise RuntimeError("phase plan must live at <plans_dir>/phase-<phase>/sprints.jsonl")
    return match.group(1)


def bead_prefix(root: str | None, start: Path) -> str:
    """The bead id prefix: taken from the phase root id (`<prefix>-phase-<p>`) when one is given,
    else `bead_prefix` from the repository configuration of the repository holding `start`."""
    if root:
        match = re.fullmatch(r"(.+)-phase-[a-z0-9]+", root)
        if not match:
            raise RuntimeError(f"root bead id {root!r} is not <prefix>-phase-<phase>")
        return match.group(1)
    return str(repo_config.load(repo_config.repo_root(start))["bead_prefix"])


def _dev_bead_id(prefix: str, sprint: str) -> str:
    if not re.fullmatch(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*", sprint):
        raise RuntimeError(f"invalid sprint name {sprint!r}")
    return f"{prefix}-{sprint}"


def load_phase_plan(path: Path, root: str | None = None) -> dict:
    """Parse the compact JSONL plan and return the normalized runtime view.

    The normalized mapping is intentionally in-memory only.  Consumers retain
    their existing pair-oriented interface while all persisted graph authority
    remains the compact tuple file.
    """
    prefix: str | None = None  # read on the first row, so an empty plan fails as empty
    rows: list[dict[str, object]] = []
    seen_sprints: set[str] = set()
    seen_sanity: set[str] = set()
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise RuntimeError(f"{path}:{number}: {exc}") from exc
        if not isinstance(value, list) or len(value) != 3:
            raise RuntimeError(f"{path}:{number}: each line must be [sprint_name, sanity_bead_id, depends_on]")
        sprint, sanity, depends_on = value
        if not isinstance(sprint, str) or not sprint:
            raise RuntimeError(f"{path}:{number}: sprint_name must be a nonempty string")
        prefix = prefix or bead_prefix(root, path.parent)
        dev = _dev_bead_id(prefix, sprint)
        if not isinstance(sanity, str) or not sanity or sanity == dev:
            raise RuntimeError(f"{path}:{number}: sanity_bead_id must be a nonempty bead ID distinct from {dev}")
        if not isinstance(depends_on, list) or not all(isinstance(item, str) and item for item in depends_on):
            raise RuntimeError(f"{path}:{number}: depends_on must be an array of sprint names")
        if sprint in seen_sprints or sanity in seen_sanity:
            raise RuntimeError(f"{path}:{number}: duplicate sprint or sanity bead")
        if sprint in depends_on or len(set(depends_on)) != len(depends_on):
            raise RuntimeError(f"{path}:{number}: depends_on contains a duplicate or self dependency")
        seen_sprints.add(sprint)
        seen_sanity.add(sanity)
        rows.append({"sprint": sprint, "dev_bead_id": dev, "sanity_bead_id": sanity, "depends_on": depends_on})
    if not rows:
        raise RuntimeError(f"{path}: phase plan contains no sprints")
    for row in rows:
        unknown = set(row["depends_on"]) - seen_sprints
        if unknown:
            raise RuntimeError(f"{path}: {row['sprint']} depends_on unknown sprint(s): {', '.join(sorted(unknown))}")
    phase = _phase_from_path(path)
    # The root is supplied by the caller when known; otherwise it is named from the
    # configured prefix and the phase directory, never stored in the tuple file.
    expected_root = root or f"{prefix}-phase-{phase}"
    by_sprint = {str(row["sprint"]): row for row in rows}
    normalized = []
    for row in rows:
        prereq_sanity = [str(by_sprint[name]["sanity_bead_id"]) for name in row["depends_on"]]
        normalized.append({"dev_bead_id": row["dev_bead_id"], "sanity_bead_id": row["sanity_bead_id"],
                           "depends_on_sanity_bead_ids": prereq_sanity})
    result = {"root_bead_id": expected_root, "sprints": normalized}
    validate_index(result)
    return result


def index_bead_pairs(index: dict) -> dict[str, str]:
    """Validate explicit identity pairs without inventing IDs from naming conventions."""
    pairs: dict[str, str] = {}
    rows = index.get("sprints")
    if not isinstance(rows, list):
        raise RuntimeError("sprint index must contain a sprints array")
    for row in rows:
        allowed = {"dev_bead_id", "sanity_bead_id", "depends_on_sanity_bead_ids"}
        if not isinstance(row, dict) or not {"dev_bead_id", "sanity_bead_id"} <= set(row) or set(row) - allowed:
            raise RuntimeError("each normalized sprint item must contain dev_bead_id, sanity_bead_id, and optional depends_on_sanity_bead_ids")
        dev, gate = row.get("dev_bead_id"), row.get("sanity_bead_id")
        if not isinstance(dev, str) or not dev:
            raise RuntimeError("dev_bead_id must be a nonempty bead ID")
        if not isinstance(gate, str) or not gate or gate == dev:
            raise RuntimeError(f"{dev}: missing or invalid sanity_bead_id; regenerate the sprint index")
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
