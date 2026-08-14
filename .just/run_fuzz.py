#!/usr/bin/env python3
"""Validate, plan, and execute bounded adversarial-fuzz campaigns.

The generic AI48 targets remain contract-only.  AN.15 adds one deliberately
small execution lane for checked template emission: four fixed test seams run
inside an approved ATM worktree.  This runner never invokes ``sc-compose`` and
never accepts arbitrary commands, so a campaign cannot become a shell-out or a
production-code editing mechanism.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import json
import re
import subprocess
import sys
from typing import Any


SCHEMA_VERSION = "adversarial-fuzzing/v1"
TARGETS = (
    "var-file",
    "frontmatter",
    "resolver",
    "renderer",
    "includes",
    "cli",
    "local-http-framing",
    "atm-template-checked-emission",
    "full",
)
WORKERS = ("shape-probe", "template-probe", "boundary-probe", "differential-probe")
WORKER_TARGETS = {
    "shape-probe": "var-file",
    "template-probe": "renderer",
    "boundary-probe": "cli",
    "differential-probe": "full",
}
CAMPAIGN_FIELDS = {
    "worktree_path",
    "target",
    "baseline_ref",
    "seed",
    "max_workers",
    "cases_per_worker",
    "per_worker_timeout_s",
    "promote_regressions",
    "campaign_id",
    "candidate_ref",
    "final_ci_commit",
    "notes",
}
WORKER_RESULT_FIELDS = {
    "correlation_id",
    "target",
    "status",
    "cases_run",
    "finding_ids",
    "error",
    "seam",
    "oracle",
    "diagnostic_codes",
}
REQUIRED_WORKER_RESULT_FIELDS = {"correlation_id", "target", "status", "cases_run", "finding_ids", "error"}
STATUSES = ("success", "failed", "timed_out")

# These commands are intentionally closed over by the repository.  Do not add
# a command field to the campaign schema: accepting a caller-provided command
# would turn a bounded assurance campaign into a shell execution interface.
CHECKED_EMISSION_WORKERS = {
    "shape-probe": {
        "seam": "Tokio/Axum template admission through the SQLite catalog and mailbox boundary",
        "oracle": "missing required input rejects before any template catalog or mailbox row is written",
        "command": ("cargo", "test", "-p", "atm-http-runtime", "an15_shape_probe_"),
    },
    "template-probe": {
        "seam": "sealed atm-template-sc-compose checked final rendering",
        "oracle": "format classification, escaping, and Unicode are deterministic",
        "command": ("cargo", "test", "-p", "atm-template-sc-compose", "an15_template_probe_"),
    },
    "boundary-probe": {
        "seam": "template catalog admission and confined include/fallback paths",
        "oracle": "rejection neither escapes the root nor leaks or mutates persisted state",
        "command": ("cargo", "test", "-p", "atm-template-sc-compose", "an15_boundary_probe_"),
    },
    "differential-probe": {
        "seam": "atm-core render-on-read from captured persisted variables",
        "oracle": "captured stored values, never ambient state, determine rendering",
        "command": ("cargo", "test", "-p", "agent-team-mail-core", "an15_differential_probe_"),
    },
}


class FuzzInputError(ValueError):
    """A campaign or worker result violates the public fuzz contract."""


def repository_root() -> Path:
    return Path(__file__).resolve().parents[1]


def _inside(path: Path, root: Path, label: str) -> Path:
    resolved = path.expanduser().resolve()
    allowed = (root.resolve(), root.resolve().parent / f"{root.resolve().name}-worktrees")
    if not any(resolved == candidate or candidate in resolved.parents for candidate in allowed):
        raise FuzzInputError(f"{label} must stay inside the repository or approved worktrees: {path}")
    return resolved


def _bounded_int(value: Any, name: str, minimum: int, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise FuzzInputError(f"{name} must be an integer between {minimum} and {maximum}")
    return value


def validate_campaign(payload: Any, root: Path | None = None) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise FuzzInputError("campaign must be a JSON object")
    unknown = set(payload) - CAMPAIGN_FIELDS
    if unknown:
        raise FuzzInputError(f"unsupported campaign fields: {', '.join(sorted(unknown))}")
    missing = (
        CAMPAIGN_FIELDS
        - {"baseline_ref", "notes", "campaign_id", "candidate_ref", "final_ci_commit"}
        - payload.keys()
    )
    if missing:
        raise FuzzInputError(f"missing campaign fields: {', '.join(sorted(missing))}")
    repo = (root or repository_root()).resolve()
    worktree_raw = payload["worktree_path"]
    if not isinstance(worktree_raw, str) or not Path(worktree_raw).is_absolute():
        raise FuzzInputError("worktree_path must be an absolute path")
    worktree = _inside(Path(worktree_raw), repo, "worktree_path")
    if not worktree.is_dir():
        raise FuzzInputError(f"worktree_path does not name an existing directory: {worktree}")
    target = payload["target"]
    if target not in TARGETS:
        raise FuzzInputError(f"target must be one of {', '.join(TARGETS)}")
    baseline = payload.get("baseline_ref")
    if baseline is not None and (not isinstance(baseline, str) or not baseline.strip() or "\n" in baseline):
        raise FuzzInputError("baseline_ref must be a non-empty single-line string when present")
    notes = payload.get("notes", "")
    if not isinstance(notes, str) or len(notes) > 4000:
        raise FuzzInputError("notes must be a string of at most 4000 characters")
    if not isinstance(payload.get("promote_regressions"), bool):
        raise FuzzInputError("promote_regressions must be boolean")
    max_workers = _bounded_int(payload["max_workers"], "max_workers", 1, 4)
    cases_per_worker = _bounded_int(payload["cases_per_worker"], "cases_per_worker", 1, 1000)
    per_worker_timeout_s = _bounded_int(
        payload["per_worker_timeout_s"], "per_worker_timeout_s", 1, 600
    )
    if target == "atm-template-checked-emission":
        if max_workers != len(WORKERS):
            raise FuzzInputError("atm-template-checked-emission requires exactly four workers")
        if cases_per_worker < 100:
            raise FuzzInputError("atm-template-checked-emission requires at least 100 cases per worker")
        if per_worker_timeout_s != 120:
            raise FuzzInputError("atm-template-checked-emission requires a 120-second worker timeout")
        for field in ("campaign_id", "candidate_ref", "final_ci_commit"):
            value = payload.get(field)
            if not isinstance(value, str) or not value.strip():
                raise FuzzInputError(
                    f"atm-template-checked-emission requires a non-empty {field}"
                )
        if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", payload["campaign_id"]):
            raise FuzzInputError("campaign_id must be a stable safe identifier")
        for field in ("candidate_ref", "final_ci_commit"):
            if not re.fullmatch(r"[0-9a-f]{40}", payload[field]):
                raise FuzzInputError(f"{field} must be a full lowercase git commit SHA")
    return {
        "worktree_path": str(worktree),
        "target": target,
        "baseline_ref": baseline,
        "seed": _bounded_int(payload["seed"], "seed", 0, 2**63 - 1),
        "max_workers": max_workers,
        "cases_per_worker": cases_per_worker,
        "per_worker_timeout_s": per_worker_timeout_s,
        "promote_regressions": payload["promote_regressions"],
        "campaign_id": payload.get("campaign_id"),
        "candidate_ref": payload.get("candidate_ref"),
        "final_ci_commit": payload.get("final_ci_commit"),
        "notes": notes,
    }


def validate_worker_result(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise FuzzInputError("worker result must be a JSON object")
    unknown = set(payload) - WORKER_RESULT_FIELDS
    if unknown:
        raise FuzzInputError(f"unsupported worker result fields: {', '.join(sorted(unknown))}")
    missing = REQUIRED_WORKER_RESULT_FIELDS - payload.keys()
    if missing:
        raise FuzzInputError(f"missing worker result fields: {', '.join(sorted(missing))}")
    correlation_id = payload["correlation_id"]
    if correlation_id not in WORKERS:
        raise FuzzInputError(f"unknown worker correlation_id: {correlation_id!r}")
    target = payload["target"]
    if not isinstance(target, str) or target not in TARGETS:
        raise FuzzInputError("worker target is invalid")
    status = payload["status"]
    if status not in STATUSES:
        raise FuzzInputError(f"worker status must be one of {', '.join(STATUSES)}")
    cases_run = _bounded_int(payload["cases_run"], "cases_run", 0, 1000)
    finding_ids = payload["finding_ids"]
    if not isinstance(finding_ids, list) or any(not isinstance(item, str) for item in finding_ids):
        raise FuzzInputError("finding_ids must be an array of strings")
    error = payload["error"]
    if error is not None and not isinstance(error, dict):
        raise FuzzInputError("worker error must be an object or null")
    seam = payload.get("seam")
    oracle = payload.get("oracle")
    diagnostic_codes = payload.get("diagnostic_codes", [])
    if seam is not None and not isinstance(seam, str):
        raise FuzzInputError("worker seam must be a string when present")
    if oracle is not None and not isinstance(oracle, str):
        raise FuzzInputError("worker oracle must be a string when present")
    if not isinstance(diagnostic_codes, list) or any(not isinstance(item, str) for item in diagnostic_codes):
        raise FuzzInputError("diagnostic_codes must be an array of strings")
    return {
        "correlation_id": correlation_id,
        "target": target,
        "status": status,
        "cases_run": cases_run,
        "finding_ids": list(finding_ids),
        "error": error,
        "seam": seam,
        "oracle": oracle,
        "diagnostic_codes": list(diagnostic_codes),
    }


def selected_workers(campaign: dict[str, Any]) -> list[str]:
    if campaign["target"] in {"full", "local-http-framing", "atm-template-checked-emission"}:
        return list(WORKERS[: campaign["max_workers"]])
    selected = [worker for worker in WORKERS if WORKER_TARGETS[worker] == campaign["target"]]
    if not selected:
        selected = ["differential-probe"]
    return selected[: campaign["max_workers"]]


def _planned_worker(campaign: dict[str, Any], correlation_id: str) -> dict[str, Any]:
    contract = CHECKED_EMISSION_WORKERS.get(correlation_id, {})
    return {
        "correlation_id": correlation_id,
        "target": campaign["target"],
        "status": "success",
        "cases_run": campaign["cases_per_worker"],
        "finding_ids": [],
        "error": None,
        "seam": contract.get("seam"),
        "oracle": contract.get("oracle"),
        "diagnostic_codes": [],
    }


def _execute_worker(campaign: dict[str, Any], correlation_id: str) -> dict[str, Any]:
    """Run one closed-over campaign worker and retain only structured diagnostics."""
    contract = CHECKED_EMISSION_WORKERS[correlation_id]
    try:
        completed = subprocess.run(
            contract["command"],
            cwd=campaign["worktree_path"],
            capture_output=True,
            text=True,
            check=False,
            timeout=campaign["per_worker_timeout_s"],
        )
    except subprocess.TimeoutExpired:
        return {
            **_planned_worker(campaign, correlation_id),
            "status": "timed_out",
            "cases_run": 0,
            "error": {"code": "worker_timeout", "owner": "campaign-maintainer"},
            "diagnostic_codes": ["worker_timeout"],
        }
    if completed.returncode != 0:
        return {
            **_planned_worker(campaign, correlation_id),
            "status": "failed",
            "cases_run": 0,
            "error": {"code": "worker_test_failure", "returncode": completed.returncode},
            "diagnostic_codes": ["worker_test_failure"],
        }
    return _planned_worker(campaign, correlation_id)


def build_result(campaign: dict[str, Any], dry_run: bool = True, execute: bool = False) -> dict[str, Any]:
    if execute and campaign["target"] != "atm-template-checked-emission":
        raise FuzzInputError("real execution is currently supported only for atm-template-checked-emission")
    if execute and dry_run:
        raise FuzzInputError("--execute and --dry-run cannot be combined")
    workers = []
    worker_ids = selected_workers(campaign)
    if execute:
        # Four workers is the public maximum and task commands are static.
        with ThreadPoolExecutor(max_workers=len(worker_ids)) as executor:
            workers = list(executor.map(lambda worker: _execute_worker(campaign, worker), worker_ids))
    else:
        workers = [_planned_worker(campaign, worker) for worker in worker_ids]
    failed_workers = sum(worker["status"] != "success" for worker in workers)
    return {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "executed" if execute else ("dry-run" if dry_run else "contract-only"),
        "campaign": campaign,
        "workers": workers,
        "findings": [],
        "promoted_tests": [],
        "unresolved_candidates": [],
        "outcome_ledger": {
            "confirmed_bug": [],
            "non_repro": [],
            "benign": [],
            "inconclusive": [],
        },
        "summary": {
            "all_successful": failed_workers == 0,
            "confirmed_bugs": 0,
            "intentional_boundaries": 0,
            "inconclusive": 0,
            "failed_workers": failed_workers,
        },
    }


def default_campaign(root: Path) -> dict[str, Any]:
    return {
        "worktree_path": str(root.resolve()),
        "target": "full",
        "baseline_ref": None,
        "seed": 157,
        "max_workers": 4,
        "cases_per_worker": 100,
        "per_worker_timeout_s": 120,
        "promote_regressions": True,
        "campaign_id": None,
        "candidate_ref": None,
        "final_ci_commit": None,
        "notes": "AI48 contract-only coordinator; real execution is restricted to AN.15 checked emission.",
    }


def load_campaign(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise FuzzInputError(f"unable to read campaign JSON: {path}") from exc


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Validate and plan a bounded adversarial-fuzz campaign.")
    parser.add_argument("--campaign", type=Path, help="campaign JSON path; defaults to the built-in contract")
    parser.add_argument("--output", type=Path, help="optional JSON output path inside the repository")
    parser.add_argument("--dry-run", action="store_true", help="emit a deterministic plan without running workers")
    parser.add_argument("--execute", action="store_true", help="run the fixed AN.15 checked-emission worker set")
    args = parser.parse_args(argv[1:])
    root = repository_root()
    try:
        raw = load_campaign(args.campaign) if args.campaign else default_campaign(root)
        campaign = validate_campaign(raw, root)
        result = build_result(campaign, dry_run=args.dry_run, execute=args.execute)
        encoded = json.dumps(result, indent=2, sort_keys=True) + "\n"
        if args.output:
            output = _inside(args.output, root, "output")
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(encoded, encoding="utf-8")
        print(encoded, end="")
        return 0
    except FuzzInputError as exc:
        print(f"fuzz: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
