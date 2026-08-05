#!/usr/bin/env python3
"""Query all open findings for one branch from the canonical triage graph.

An "open" finding is defined exactly the same way the live QA/merge gate
defines it (see ``open-findings-for-sprint.sparql``): its occurrence on the
requested branch is neither ``closed`` nor carries a terminal status (fixed,
deferred, waived, etc.), and no ``triage:Resolution`` record resolves it.

This script deliberately reuses the shared graph loader/query runner from the
graph-orchestration skill (``query_runner.py``) instead of re-implementing
Turtle loading or finding-scope filtering, so results here always agree with
what the live merge/dispatch gate sees.

Safety: findings only live in ``integrate/phase-*`` worktrees (sprint
worktrees carry copied, potentially stale TTL). This script therefore refuses
to run unless either (a) the current working directory's git branch begins
with ``integrate``, or (b) ``--integration-root`` explicitly names a path
whose own branch begins with ``integrate``. This is a hard guard, not a
default, precisely so a stale sprint-worktree copy is never queried by
accident.

A finding's ``triage:status`` and ``triage:Resolution`` records only change
once QA/team-lead verify and close it -- not the moment a fix is committed
and pushed. So a finding this script already reported will keep showing up
on every subsequent call, even after it has been fixed, until QA closes it
upstream. A caller that loops over results (e.g. a dev fix loop) is
responsible for tracking which finding IDs it has already pushed a fix for
and diffing that against each fresh call's results itself (see the
closing-triage SKILL.md) -- this script always reports the canonical live
query result and does not track caller-side progress.

Usage:
    python3 query_open_findings.py --branch feature/pAJ-s6-runtime-observation-snapshot
    python3 query_open_findings.py --branch <name> --integration-root /path/to/integrate-worktree
    python3 query_open_findings.py --branch <name> --phase AJ --json
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

try:
    from rdflib import Literal
    _RDFLIB_ERROR: str | None = None
except ImportError as exc:  # pragma: no cover - environment error
    Literal = None  # type: ignore[assignment]
    _RDFLIB_ERROR = str(exc)


class QueryError(RuntimeError):
    """An operational error which prevents a trustworthy query result."""


def _git(cwd: Path, *args: str) -> str:
    try:
        result = subprocess.run(
            ["git", *args],
            cwd=str(cwd),
            text=True,
            capture_output=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        detail = getattr(exc, "stderr", "") or str(exc)
        raise QueryError(f"git {' '.join(args)} failed: {detail.strip()}") from exc
    return result.stdout.strip()


def _current_branch(cwd: Path) -> str:
    return _git(cwd, "branch", "--show-current")


def resolve_integration_root(explicit_root: Path | None, cwd: Path) -> Path:
    """Return a worktree root whose branch begins with 'integrate'.

    Findings only live in integrate/phase-* worktrees. Refuse to guess: either
    the caller is already standing in such a worktree, or they must say
    exactly which one to use via --integration-root.
    """
    if explicit_root is not None:
        root = explicit_root.resolve()
        if not root.is_dir():
            raise QueryError(f"--integration-root does not exist: {root}")
        branch = _current_branch(root)
        if not branch.startswith("integrate"):
            raise QueryError(
                f"--integration-root {root} is on branch {branch!r}, which does not "
                "begin with 'integrate'. Point --integration-root at an "
                "integrate/phase-* worktree."
            )
        return root

    branch = _current_branch(cwd)
    if not branch.startswith("integrate"):
        raise QueryError(
            f"current branch {branch!r} does not begin with 'integrate'. "
            "Run this script from an integrate/phase-* worktree, or pass "
            "--integration-root /path/to/integrate-worktree to specify one "
            "explicitly."
        )
    return Path(_git(cwd, "rev-parse", "--show-toplevel"))


def _phase_dir(root: Path, requested: str | None) -> tuple[str, Path]:
    sprints = root / ".sprints"
    if requested:
        phase = requested.removeprefix("phase-")
        path = sprints / phase
        if not (path / "structure.ttl").is_file():
            raise QueryError(f"missing phase structure: {path / 'structure.ttl'}")
        return phase, path
    candidates = sorted(p.parent for p in sprints.glob("*/structure.ttl"))
    if len(candidates) != 1:
        names = ", ".join(p.name for p in candidates) or "none"
        raise QueryError(
            f"cannot determine a unique phase under {sprints}; found {len(candidates)} "
            f"({names}); pass --phase"
        )
    return candidates[0].name, candidates[0]


def _graph_runner(script_dir: Path):
    """Load graph-orchestration's public graph/query helpers once.

    Reusing this module (rather than a parallel Turtle loader) guarantees
    this script's notion of "open" always matches the live merge/dispatch
    gate's notion of "open".
    """
    runner_path = (
        script_dir.resolve().parents[1] / "graph-orchestration" / "scripts" / "query_runner.py"
    )
    if not runner_path.is_file():
        raise QueryError(f"cannot find graph query runner: {runner_path}")
    spec = importlib.util.spec_from_file_location("closing_triage_graph_runner", runner_path)
    if spec is None or spec.loader is None:
        raise QueryError(f"cannot load graph query runner: {runner_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def query_open_findings(
    root: Path,
    branch: str,
    phase: str | None,
    script_dir: Path,
) -> list[dict[str, Any]]:
    if _RDFLIB_ERROR:
        raise QueryError(f"rdflib is required; install it with pip install rdflib ({_RDFLIB_ERROR})")

    phase_name, phase_path = _phase_dir(root, phase)
    runner = _graph_runner(script_dir)
    try:
        source = runner.resolve_phase_source(phase_name, str(phase_path))
    except Exception as exc:  # noqa: BLE001 - normalize shared source errors
        raise QueryError(f"could not resolve current integration phase source: {exc}") from exc

    try:
        graph = runner.load_graph(str(source.ttl_dir), findings_dir=source.findings_dir)
    except Exception as exc:  # noqa: BLE001 - normalize graph runner failures
        raise QueryError(f"could not load finding graph: {exc}") from exc

    query_path = script_dir.resolve().parents[1] / "graph-orchestration" / "scripts" / "open-findings-for-sprint.sparql"
    if not query_path.is_file():
        raise QueryError(f"cannot find shared query file: {query_path}")

    try:
        rows = runner.run_sparql(graph, query_path, {"BRANCH": Literal(branch)})
    except Exception as exc:  # noqa: BLE001 - normalize graph runner failures
        raise QueryError(f"query failed: {exc}") from exc

    # open-findings-for-sprint.sparql already orders by severity (blocking,
    # then important, then minor; invalid severities sort first, fail-closed)
    # and, within a severity, by foundAt. Preserve that order as-is.
    findings: list[dict[str, Any]] = []
    for row in rows:
        finding_uri, finding_id, severity, raw_severity, status, found_at, description = row
        findings.append(
            {
                "finding": str(finding_uri),
                "finding_id": str(finding_id) if finding_id is not None else str(finding_uri).rsplit(":", 1)[-1],
                "severity": str(severity),
                "raw_severity": str(raw_severity),
                "status": str(status) if status is not None else None,
                "found_at": str(found_at),
                "description": str(description),
            }
        )
    return findings


def _print_table(branch: str, findings: list[dict[str, Any]]) -> None:
    if not findings:
        print(f"No open findings for branch {branch!r}.")
        return
    print(f"Open findings for branch {branch!r} ({len(findings)}):")
    print()
    for item in findings:
        status = item["status"] or "open"
        print(f"- [{item['severity'].upper()}] {item['finding_id']} (status: {status})")
        print(f"    found_at: {item['found_at']}")
        description = item["description"]
        if len(description) > 200:
            description = description[:197] + "..."
        print(f"    {description}")
        print()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--branch", required=True, help="branch name to query open findings for")
    parser.add_argument(
        "--integration-root",
        type=Path,
        default=None,
        help="path to an integrate/phase-* worktree; required if the current directory's "
        "branch does not begin with 'integrate'",
    )
    parser.add_argument("--phase", default=None, help="phase name (e.g. AJ); auto-detected if only one exists")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of a table")
    args = parser.parse_args(argv)

    try:
        root = resolve_integration_root(args.integration_root, Path.cwd())
        findings = query_open_findings(root, args.branch, args.phase, Path(__file__).parent)
    except QueryError as exc:
        payload = {"kind": "error", "message": str(exc)}
        if args.json:
            print(json.dumps(payload, sort_keys=True))
        else:
            print(f"error: {exc}", file=sys.stderr)
        return 2

    if args.json:
        print(json.dumps({"branch": args.branch, "count": len(findings), "findings": findings}, indent=2, sort_keys=True))
    else:
        _print_table(args.branch, findings)
    return 0


if __name__ == "__main__":
    sys.exit(main())
