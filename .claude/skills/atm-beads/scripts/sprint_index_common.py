"""Shared process and phase-path helpers for sprint planning and reporting."""
from __future__ import annotations
import json
import re
import subprocess
from pathlib import Path
from typing import Any


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
    if not re.fullmatch(r"[a-z0-9]+", phase):
        raise RuntimeError(f"invalid phase: {phase}")
    return repo / "docs" / "plans" / f"phase-{phase}" / "sprints.json"


def index_bead_pairs(index: dict) -> dict[str, str]:
    """Validate explicit identity pairs without inventing IDs from naming conventions."""
    pairs: dict[str, str] = {}
    rows = index.get("sprints")
    if not isinstance(rows, list):
        raise RuntimeError("sprint index must contain a sprints array")
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"dev_bead_id", "sanity_bead_id"}:
            raise RuntimeError("each sprint index item must contain only dev_bead_id and sanity_bead_id")
        dev, gate = row.get("dev_bead_id"), row.get("sanity_bead_id")
        if not isinstance(dev, str) or not dev:
            raise RuntimeError("dev_bead_id must be a nonempty bead ID")
        if not isinstance(gate, str) or not gate or gate == dev:
            raise RuntimeError(f"{dev}: missing or invalid sanity_bead_id; regenerate the sprint index")
        if dev in pairs or gate in pairs.values():
            raise RuntimeError(f"{dev}: duplicate dev or sanity bead in sprint index")
        pairs[dev] = gate
    if set(pairs) & set(pairs.values()):
        raise RuntimeError("a sanity bead cannot also be a sprint dev bead")
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
