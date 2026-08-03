#!/usr/bin/env python3
"""Validate and plan bounded adversarial-fuzz campaigns.

AI48 deliberately stops at the coordinator/probe contract. It does not invoke
product parsers, mutate a worktree, or render reports; later sprints own those
execution and publication concerns.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import json
import sys
from typing import Any


SCHEMA_VERSION = "adversarial-fuzzing/v1"
TARGETS = ("var-file", "frontmatter", "resolver", "renderer", "includes", "cli", "local-http-framing", "full")
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
    "notes",
}
WORKER_RESULT_FIELDS = {"correlation_id", "target", "status", "cases_run", "finding_ids", "error"}
STATUSES = ("success", "failed", "timed_out")


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
    missing = CAMPAIGN_FIELDS - {"baseline_ref", "notes"} - payload.keys()
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
    return {
        "worktree_path": str(worktree),
        "target": target,
        "baseline_ref": baseline,
        "seed": _bounded_int(payload["seed"], "seed", 0, 2**63 - 1),
        "max_workers": _bounded_int(payload["max_workers"], "max_workers", 1, 4),
        "cases_per_worker": _bounded_int(payload["cases_per_worker"], "cases_per_worker", 1, 1000),
        "per_worker_timeout_s": _bounded_int(payload["per_worker_timeout_s"], "per_worker_timeout_s", 1, 600),
        "promote_regressions": payload["promote_regressions"],
        "notes": notes,
    }


def validate_worker_result(payload: Any) -> dict[str, Any]:
    if not isinstance(payload, dict):
        raise FuzzInputError("worker result must be a JSON object")
    unknown = set(payload) - WORKER_RESULT_FIELDS
    if unknown:
        raise FuzzInputError(f"unsupported worker result fields: {', '.join(sorted(unknown))}")
    missing = WORKER_RESULT_FIELDS - payload.keys()
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
    return {
        "correlation_id": correlation_id,
        "target": target,
        "status": status,
        "cases_run": cases_run,
        "finding_ids": list(finding_ids),
        "error": error,
    }


def selected_workers(campaign: dict[str, Any]) -> list[str]:
    if campaign["target"] in {"full", "local-http-framing"}:
        return list(WORKERS[: campaign["max_workers"]])
    selected = [worker for worker in WORKERS if WORKER_TARGETS[worker] == campaign["target"]]
    if not selected:
        selected = ["differential-probe"]
    return selected[: campaign["max_workers"]]


def build_result(campaign: dict[str, Any], dry_run: bool = True) -> dict[str, Any]:
    workers = []
    for correlation_id in selected_workers(campaign):
        workers.append(
            {
                "correlation_id": correlation_id,
                "target": campaign["target"],
                "status": "success",
                "cases_run": campaign["cases_per_worker"],
                "finding_ids": [],
                "error": None,
            }
        )
    return {
        "schema_version": SCHEMA_VERSION,
        "execution_mode": "dry-run" if dry_run else "contract-only",
        "campaign": campaign,
        "workers": workers,
        "findings": [],
        "promoted_tests": [],
        "unresolved_candidates": [],
        "summary": {
            "all_successful": True,
            "confirmed_bugs": 0,
            "intentional_boundaries": 0,
            "inconclusive": 0,
            "failed_workers": 0,
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
        "notes": "AI48 contract-only coordinator; real campaign execution is deferred.",
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
    args = parser.parse_args(argv[1:])
    root = repository_root()
    try:
        raw = load_campaign(args.campaign) if args.campaign else default_campaign(root)
        campaign = validate_campaign(raw, root)
        result = build_result(campaign, dry_run=args.dry_run)
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
