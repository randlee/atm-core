#!/usr/bin/env python3
"""Query all open findings for one sprint branch from the canonical triage graph.

An "open" finding is one the live QA/merge gate would still count against
your sprint (see ``open-findings-for-sprint.sparql``, origin-sprint mode):
it was found in the sprint that owns the requested branch
(``triage:foundIn``), carries no terminal ``triage:status`` (fixed,
deferred, waived, etc.), and no ``triage:Resolution`` record resolves it.

Scoping is by origin sprint, not by branch occurrence: findings from
earlier sprints whose defects merely propagate onto this branch's checkout
belong to their origin sprint's developer and arrive fixed via merge --
they are deliberately excluded here.

This script deliberately reuses the shared graph loader/query runner from the
graph-orchestration skill (``query_runner.py``) instead of re-implementing
Turtle loading or finding-scope filtering, so results here always agree with
what the live merge/dispatch gate sees.

Safety: findings only live in integration worktrees -- branch beginning with
``integrat`` (covers both ``integrate/*`` and ``integration/*``) -- while
sprint worktrees carry copied, potentially stale TTL; this script only ever
queries an integration worktree. Run it from your sprint worktree: it
auto-discovers the sibling integration worktree via ``git worktree list``,
failing closed (and requiring ``--integration-root``) unless exactly one
exists.

Note: a finding stays "open" here until QA closes it upstream -- not when a
fix is pushed -- so a looping caller must track its own already-fixed set
(see the closing-triage SKILL.md).

Usage (from your sprint worktree; integrate worktree is auto-discovered):
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
    from rdflib import Graph, Namespace, RDF, URIRef
    _RDFLIB_ERROR: str | None = None
except ImportError as exc:  # pragma: no cover - environment error
    Graph = Namespace = RDF = URIRef = None  # type: ignore[assignment]
    _RDFLIB_ERROR = str(exc)

TRIAGE = Namespace("urn:atm:triage:") if Namespace else None


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
    """Return a worktree root whose branch begins with 'integrat'.

    Findings only live in integration worktrees (branch prefix ``integrat``,
    matching both integrate/* and integration/*). Resolution order:
    1. An explicit --integration-root (validated to be on an integrat* branch).
    2. The current worktree, if it is already on an integrat* branch.
    3. Auto-discovery via ``git worktree list --porcelain``: succeeds only if
       exactly one worktree of this repo is on an integrat* branch; zero or
       multiple candidates fail closed and require --integration-root.
    A sprint worktree's own (potentially stale) triage copy is never queried.
    """
    if explicit_root is not None:
        root = explicit_root.resolve()
        if not root.is_dir():
            raise QueryError(f"--integration-root does not exist: {root}")
        branch = _current_branch(root)
        if not branch.startswith("integrat"):
            raise QueryError(
                f"--integration-root {root} is on branch {branch!r}, which does not "
                "begin with 'integrat'. Point --integration-root at an "
                "integrate/phase-* (or integration/*) worktree."
            )
        return root

    branch = _current_branch(cwd)
    if branch.startswith("integrat"):
        return Path(_git(cwd, "rev-parse", "--show-toplevel"))

    # Auto-discover the sibling integration worktree from a sprint worktree.
    # Same worktree-walk pattern as triage-report's discover_integration_root:
    # fail closed unless exactly one integrat* worktree exists. The prefix is
    # 'integrat' (not 'integrate') so both integrate/* and integration/*
    # branch spellings match.
    candidates: list[tuple[Path, str]] = []
    current_path: Path | None = None
    current_branch: str | None = None
    for line in _git(cwd, "worktree", "list", "--porcelain").splitlines() + [""]:
        if line.startswith("worktree "):
            current_path = Path(line.removeprefix("worktree "))
        elif line.startswith("branch refs/heads/"):
            current_branch = line.removeprefix("branch refs/heads/")
        elif not line.strip():
            if (
                current_path is not None
                and current_branch is not None
                and current_branch.startswith("integrat")
            ):
                candidates.append((current_path, current_branch))
            current_path = current_branch = None
    if len(candidates) != 1:
        names = ", ".join(f"{path} ({br})" for path, br in candidates) or "none"
        raise QueryError(
            f"current branch {branch!r} does not begin with 'integrat' and "
            f"auto-discovery found {len(candidates)} integration worktree(s) "
            f"({names}); pass --integration-root /path/to/integrate-worktree "
            "to name one explicitly."
        )
    return candidates[0][0]


def _branch_from_criteria(criteria: str) -> str | None:
    """Derive the documented sprint-branch convention from a criteria path.

    ``triage:branch`` is preferred when a phase records it explicitly; this
    fallback (same as triage-report's) keeps older phase records usable.
    """
    match = re.fullmatch(
        r"sprint-([a-z][a-z0-9]*)-([0-9]+)(?:-(pre))?-(.+)",
        Path(criteria).stem,
    )
    if not match:
        return None
    prefix, number, suffix, slug = match.groups()
    return f"feature/p{prefix.upper()}-s{number}{suffix or ''}-{slug}"


def _sprint_for_branch(
    root: Path, branch: str, requested_phase: str | None
) -> tuple[str, Path, "URIRef"]:
    """Map the sprint branch to its declaring phase and sprint IRI.

    Scans ``.sprints/*/structure.ttl`` for a ``triage:Sprint`` whose
    ``triage:branch`` (or branch derived from its criteria filename, for
    older phases without an explicit branch) equals the requested branch.
    Exactly one match is required across the searched phases; anything else
    fails closed. This mapping is what scopes results to the branch's own
    sprint (``triage:foundIn``) rather than to every finding whose defect
    happens to occur on the branch's checkout.
    """
    sprints_dir = root / ".sprints"
    if requested_phase:
        phase = requested_phase.removeprefix("phase-")
        candidates = [sprints_dir / phase]
        if not (candidates[0] / "structure.ttl").is_file():
            raise QueryError(f"missing phase structure: {candidates[0] / 'structure.ttl'}")
    else:
        candidates = sorted(p.parent for p in sprints_dir.glob("*/structure.ttl"))
        if not candidates:
            raise QueryError(f"no phase structures found under {sprints_dir}")

    matches: list[tuple[str, Path, URIRef]] = []
    for phase_path in candidates:
        structure = Graph()
        structure_path = phase_path / "structure.ttl"
        try:
            structure.parse(structure_path, format="turtle")
        except Exception as exc:  # noqa: BLE001 - convert parser failures
            raise QueryError(f"{structure_path}: malformed Turtle ({exc})") from exc
        for sprint in structure.subjects(RDF.type, TRIAGE.Sprint):
            declared = [str(value) for value in structure.objects(sprint, TRIAGE.branch)]
            if branch in declared:
                matches.append((phase_path.name, phase_path, sprint))
            elif not declared:
                criteria = next(structure.objects(sprint, TRIAGE.criteria), None)
                if criteria is not None and _branch_from_criteria(str(criteria)) == branch:
                    matches.append((phase_path.name, phase_path, sprint))
    if len(matches) != 1:
        searched = ", ".join(path.name for path in candidates)
        found = ", ".join(f"{phase}:{sprint}" for phase, _, sprint in matches) or "none"
        raise QueryError(
            f"branch {branch!r} must map to exactly one declared sprint; searched "
            f"phase(s) [{searched}] under {sprints_dir} and found {len(matches)} "
            f"({found}). Is this branch part of the current integration phase? "
            "Pass --phase to narrow the search, or fix the phase structure."
        )
    return matches[0]


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

    phase_name, phase_path, sprint = _sprint_for_branch(root, branch, phase)
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
        # Origin-sprint mode: bind SPRINT only. Binding BRANCH would switch
        # the shared query to occurrence-level scoping, which returns every
        # finding whose defect propagates onto this branch's checkout --
        # including upstream sprints' findings that are not this branch's
        # work (see module docstring).
        rows = runner.run_sparql(graph, query_path, {"SPRINT": sprint})
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
        help="path to an integrate* worktree; overrides auto-discovery, and is required "
        "only when auto-discovery finds zero or multiple integrate worktrees",
    )
    parser.add_argument(
        "--phase",
        default=None,
        help="phase name (e.g. AJ); narrows the branch-to-sprint search when the "
        "branch is declared in more than one phase structure (normally auto-detected)",
    )
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
