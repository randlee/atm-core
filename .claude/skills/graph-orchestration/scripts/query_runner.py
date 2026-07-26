#!/usr/bin/env python3
"""
query_runner.py — Cursor resolution for graph-orchestration.

Called by next-dev-task. Returns JSON to stdout; errors to stderr.

Output shape (TRAVERSAL):
  {
    "phase": "TRAVERSAL",
    "vars": {
      "sprint": "PhaseF-S1",      # local sprint label
      "sprint_iri": "...",
      "sprint_order": 1,
      "criteria_doc": "..."
    }
  }

Output shape (AWAITING):
  {
    "phase": "AWAITING",
    "vars": {},
    "_incomplete_sprints": ["urn:atm:triage:PhaseF-S1", ...]
  }

Output shape (CLEANUP):
  {
    "phase": "CLEANUP",
    "vars": {}
  }

Output shape (DONE):
  {
    "phase": "DONE",
    "vars": {}
  }

Findings are NOT resolved here. The orchestrator checks .triage/ via
triaging-findings skill to decide between dev-task.xml.j2 and dev-fix.xml.j2.
Assignments are appended to events.ttl by the orchestrator after dispatch.

Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> [--validate-only]
  PHASE_LOCAL  e.g. "F"
  TTL_DIR      path to .sprints/<PHASE>/ directory on integrate/phase-N branch
  SCRIPT_DIR   path to this script's directory (for .sparql files)
  --validate-only  validate structure.ttl/events.ttl and stop before cursor resolution
"""

import importlib.util
import sys
import json
import re
import subprocess
from dataclasses import dataclass
from pathlib import Path

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF
except ImportError:
    print("ERROR: rdflib not installed. Run: pip3 install rdflib", file=sys.stderr)
    sys.exit(1)

TRIAGE_BASE = "urn:atm:triage:"
TRIAGE = Namespace(TRIAGE_BASE)


class ValidationGateError(RuntimeError):
    """A findings validation result that blocks dispatch but is reportable."""

    def __init__(self, kind: str, message: str, diagnostics: list[str], exit_code: int):
        super().__init__(message)
        self.kind = kind
        self.message = message
        self.diagnostics = diagnostics
        self.exit_code = exit_code


def _error_payload(
    code: str,
    message: str,
    *,
    diagnostics: list[str] | None = None,
    kind: str = "error",
) -> dict:
    """Return the stable JSON union arm used by every CLI error path."""

    return {
        "schema": "graph-orchestration/v1",
        "kind": kind,
        "error_code": code,
        "message": message,
        "diagnostics": list(diagnostics or []),
        "dispatch_blocked": True,
    }


def _find_repo_root(start: Path) -> Path | None:
    """Walk up from ``start`` to the repository's ``.triage`` directory."""
    current = start.resolve()
    for _ in range(10):  # max 10 levels up
        if (current / ".triage").exists():
            return current
        parent = current.parent
        if parent == current:
            break
        current = parent
    return None


@dataclass(frozen=True)
class PhaseSource:
    """Canonical current-integration inputs for one graph phase."""

    root: Path
    ttl_dir: Path
    findings_dir: Path
    branch: str | None


def _integration_worktrees(repo_root: Path, phase_dir_name: str) -> list[tuple[Path, str]]:
    """Return integration worktrees that own ``.sprints/<phase_dir_name>``."""
    try:
        result = subprocess.run(
            ["git", "worktree", "list", "--porcelain"],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return []

    matches: list[tuple[Path, str]] = []
    worktree: Path | None = None
    branch: str | None = None
    for line in [*result.stdout.splitlines(), ""]:
        if line.startswith("worktree "):
            worktree = Path(line.removeprefix("worktree "))
        elif line.startswith("branch refs/heads/"):
            branch = line.removeprefix("branch refs/heads/")
        elif not line.strip():
            if (
                worktree is not None
                and branch is not None
                and branch.startswith("integrate/phase-")
                and (worktree / ".sprints" / phase_dir_name / "structure.ttl").is_file()
            ):
                matches.append((worktree.resolve(), branch))
            worktree = None
            branch = None
    return matches


def _project_phase_from_ttl(ttl_dir: Path, fallback: str) -> str:
    """Derive the plan phase only for isolated non-git test fixtures."""
    graph = Graph()
    try:
        graph.parse(ttl_dir / "structure.ttl", format="turtle")
    except Exception:  # The normal structure gate reports this after resolution.
        return f"phase-{fallback}"
    phases = {
        match.group(1)
        for criteria in graph.objects(None, TRIAGE.criteria)
        if (match := re.search(r"docs/plans/(phase-[^/]+)/", str(criteria)))
    }
    return phases.pop() if len(phases) == 1 else f"phase-{fallback}"


def resolve_phase_source(phase_local: str, requested_ttl_dir: str) -> PhaseSource:
    """Resolve a phase to its sole current ``integrate/phase-*`` worktree.

    Sprint worktrees are never a query source: their copied TTL can be stale.
    The integration branch name is also the project triage namespace, so this
    resolver selects exactly ``.triage/<phase-name>/findings`` rather than
    scanning every historical phase.  Non-git unit fixtures retain a direct
    local fallback using ``phase-<PHASE_LOCAL>``.
    """
    requested = Path(requested_ttl_dir).resolve()
    if not (requested / "structure.ttl").is_file():
        raise RuntimeError(f"structure.ttl not found at {requested / 'structure.ttl'}")
    repo_root = _find_repo_root(requested)
    if repo_root is None:
        raise RuntimeError("cannot locate repository root containing .triage")

    candidates = _integration_worktrees(repo_root, requested.name)
    if candidates:
        if len(candidates) != 1:
            names = ", ".join(f"{branch} ({path})" for path, branch in candidates)
            raise RuntimeError(
                f"cannot determine one current integration source for {requested.name}: {names}"
            )
        root, branch = candidates[0]
        phase_name = branch.rsplit("/", 1)[-1]
        return PhaseSource(
            root=root,
            ttl_dir=root / ".sprints" / requested.name,
            findings_dir=root / ".triage" / phase_name / "findings",
            branch=branch,
        )

    # Test fixtures are deliberately not git worktrees.  Do not make a
    # production branch silently fall back to its potentially stale TTL.
    try:
        subprocess.run(
            ["git", "rev-parse", "--is-inside-work-tree"],
            cwd=repo_root,
            text=True,
            capture_output=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        project_phase = _project_phase_from_ttl(requested, phase_local)
        return PhaseSource(
            root=repo_root,
            ttl_dir=requested,
            findings_dir=repo_root / ".triage" / project_phase / "findings",
            branch=None,
        )
    raise RuntimeError(
        f"no integrate/phase-* worktree owns .sprints/{requested.name}; refusing stale branch data"
    )


def load_graph(
    ttl_dir: str | Path,
    *,
    findings_dir: Path | None = None,
    include_findings: bool = True,
) -> Graph:
    g = Graph()
    base = Path(ttl_dir)
    structure = base / "structure.ttl"
    events = base / "events.ttl"
    if not structure.exists():
        print(f"ERROR: structure.ttl not found at {structure}", file=sys.stderr)
        sys.exit(1)
    g.parse(str(structure), format="turtle")
    if events.exists():
        g.parse(str(events), format="turtle")

    known_sprints = set(g.subjects(RDF.type, TRIAGE.Sprint))

    if not include_findings:
        return g

    if findings_dir is None:
        repo_root = _find_repo_root(base)
        if repo_root is None:
            raise RuntimeError("cannot locate repository root containing .triage")
        findings_dir = repo_root / ".triage" / f"phase-{base.name}" / "findings"
    if findings_dir.is_dir():
        for findings_file in sorted(findings_dir.glob("*.ttl")):
            file_graph = Graph()
            try:
                file_graph.parse(str(findings_file), format="turtle")
            except Exception as exc:  # noqa: BLE001 - isolate one bad file, keep loading others
                print(
                    f"WARNING: skipping malformed findings file {findings_file}: {exc}",
                    file=sys.stderr,
                )
                continue

            in_scope_findings = {
                finding
                for finding in file_graph.subjects(TRIAGE.foundIn, None)
                if file_graph.value(finding, TRIAGE.foundIn) in known_sprints
            }
            if not in_scope_findings:
                continue
            for finding in in_scope_findings:
                # Keep occurrence/worktree provenance with the finding.  The
                # live triage report uses occurrence branch/status to scope a
                # gate, and dropping these linked records here would make the
                # SPARQL query see no branch occurrence at all.
                pending = [finding]
                linked: set = set()
                linked_predicates = {
                    TRIAGE.hasOccurrence,
                    TRIAGE.occursIn,
                    TRIAGE.openOn,
                    TRIAGE.promoteTo,
                    TRIAGE.closedOn,
                }
                while pending:
                    subject = pending.pop()
                    if subject in linked:
                        continue
                    linked.add(subject)
                    for predicate, obj in file_graph.predicate_objects(subject):
                        g.add((subject, predicate, obj))
                        if predicate in linked_predicates and isinstance(obj, URIRef):
                            pending.append(obj)
                for resolution in file_graph.subjects(TRIAGE.resolves, finding):
                    for triple in file_graph.triples((resolution, None, None)):
                        g.add(triple)
    return g


def run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    results = g.query(sparql_file.read_text(), initBindings=bindings)
    return list(results)


def _cli_load_graph(source: PhaseSource, *, include_findings: bool) -> Graph:
    """Load a graph while keeping malformed input errors CLI-friendly."""
    try:
        return load_graph(
            source.ttl_dir,
            findings_dir=source.findings_dir,
            include_findings=include_findings,
        )
    except SystemExit:
        message = f"query runner could not load graph at {source.ttl_dir}"
        print(json.dumps(_error_payload("graph_load", message)))
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        message = f"query runner failed to load graph: {exc}"
        print(f"ERROR: {message}", file=sys.stderr)
        print(json.dumps(_error_payload("graph_load", message)))
        raise SystemExit(1) from exc


def _cli_run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    """Run one bundled query while keeping query errors one-line."""
    try:
        return run_sparql(g, sparql_file, bindings)
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        message = f"query runner failed to run {sparql_file}: {exc}"
        print(f"ERROR: {message}", file=sys.stderr)
        print(json.dumps(_error_payload("sparql_query", message)))
        raise SystemExit(1) from exc


def _load_validator(script_dir: Path):
    """Load the canonical findings validator without duplicating its rules."""

    validator_path = script_dir / "validate-findings.py"
    if not validator_path.exists():
        raise RuntimeError(f"validate-findings.py not found at {validator_path}")
    spec = importlib.util.spec_from_file_location("graph_orchestration_validator", validator_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"unable to load validator at {validator_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def _validate_findings_before_query(source: PhaseSource, script_dir: Path) -> None:
    """Run raw findings validation before *any* graph query.

    ``load_graph`` intentionally scopes findings by ``foundIn``.  Running this
    gate first prevents malformed or incomplete records from disappearing in
    that scope filter.  A normal validation failure (exit-1 equivalent) and a
    validator execution error both stop cursor resolution; warnings alone are
    allowed by the validator's discriminated result contract.
    """

    validator = _load_validator(script_dir)
    structure = source.ttl_dir / "structure.ttl"
    events_path = source.ttl_dir / "events.ttl"
    events = events_path if events_path.exists() else None
    if source.findings_dir.is_dir():
        result = validator.run_validation(
            findings_dir=source.findings_dir,
            structure=structure,
            events=events,
            script_dir=script_dir,
        )
        if result.kind == "validation:pass":
            return
        summary = getattr(result, "summary", None)
        counts = (
            f" ({summary.errors} error(s), {summary.warnings} warning(s))"
            if summary is not None
            else ""
        )
        print(
            f"ERROR: findings validation blocked query resolution for {source.findings_dir}"
            f"{counts}",
            file=sys.stderr,
        )
        for diagnostic in result.diagnostics[:20]:
            print(diagnostic, file=sys.stderr)
        if len(result.diagnostics) > 20:
            print(
                f"… {len(result.diagnostics) - 20} diagnostic line(s) truncated; "
                "run validate-findings.py directly for the full report",
                file=sys.stderr,
            )
        if result.kind == "error":
            raise ValidationGateError(
                "error",
                "findings validation could not run",
                list(result.diagnostics),
                2,
            )
        raise ValidationGateError(
            "validation:fail",
            "findings validation failed",
            list(result.diagnostics),
            1,
        )


def main():
    if len(sys.argv) not in (4, 5) or (
        len(sys.argv) == 5 and sys.argv[4] != "--validate-only"
    ):
        message = (
            "Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> "
            "[--validate-only]"
        )
        print(
            message,
            file=sys.stderr,
        )
        print(json.dumps(_error_payload("usage", message)))
        sys.exit(1)

    phase_local = sys.argv[1]
    ttl_dir = sys.argv[2]
    script_dir = Path(sys.argv[3])
    validate_only = len(sys.argv) == 5

    phase_iri = URIRef(f"{TRIAGE_BASE}Phase{phase_local}")
    # This is deliberately before structure loading and before --validate-only:
    # every query_runner entry point must prove that raw findings are valid.
    try:
        source = resolve_phase_source(phase_local, ttl_dir)
        _validate_findings_before_query(source, script_dir)
    except ValidationGateError as exc:
        print(
            json.dumps(
                _error_payload(
                    "findings_validation",
                    exc.message,
                    diagnostics=exc.diagnostics,
                    kind=exc.kind,
                )
            )
        )
        raise SystemExit(exc.exit_code)
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        message = f"findings validation could not run: {exc}"
        print(f"ERROR: {message}", file=sys.stderr)
        print(json.dumps(_error_payload("findings_validation", message)))
        raise SystemExit(2) from exc

    # The raw findings gate above runs before structure validation so malformed
    # or incomplete records cannot disappear during phase membership filtering.
    # Once that gate passes, structure validation can report graph-shape errors
    # without being conflated with finding-schema diagnostics.
    g = _cli_load_graph(source, include_findings=not validate_only)

    # ── Validate structure before cursor ─────────────────────────────────────
    validate_rows = _cli_run_sparql(
        g, script_dir / "validate-structure.sparql", {"PHASE": phase_iri}
    )
    if validate_rows:
        diagnostics = [
            f"structure violation: {row[0]} — {row[1]}" for row in validate_rows
        ]
        for row in validate_rows:
            print(f"ERROR: structure violation: {row[0]} — {row[1]}", file=sys.stderr)
        print(
            json.dumps(
                _error_payload(
                    "structure_validation",
                    "structure validation failed",
                    diagnostics=diagnostics,
                    kind="validation:fail",
                )
            )
        )
        sys.exit(1)

    if validate_only:
        print(json.dumps({"phase": "VALIDATE_ONLY", "vars": {}}, indent=2))
        return

    # ── Cursor ───────────────────────────────────────────────────────────────
    cursor_rows = _cli_run_sparql(g, script_dir / "cursor.sparql", {"PHASE": phase_iri})

    if cursor_rows:
        sprint_iri = str(cursor_rows[0][0])
        order = int(cursor_rows[0][1])
        criteria = str(cursor_rows[0][2])
        sprint_local = sprint_iri.split(":")[-1] if ":" in sprint_iri else sprint_iri
        finding_rows = _cli_run_sparql(
            g,
            script_dir / "open-findings-for-sprint.sparql",
            {"SPRINT": URIRef(sprint_iri)},
        )
        findings = [
            {
                "finding_iri": str(row[0]),
                "finding_id": str(row[1]) if row[1] is not None else None,
                "severity": str(row[2]),
                "raw_severity": str(row[3]),
                "status": str(row[4]) if row[4] is not None else None,
                "found_at": str(row[5]),
                "description": str(row[6]),
            }
            for row in finding_rows
        ]

        invalid_findings = [
            finding for finding in findings if finding["severity"] == "invalid"
        ]
        if invalid_findings:
            print(json.dumps({
                "phase": "INVALID_FINDING_SEVERITY",
                "vars": {
                    "sprint": sprint_local,
                    "sprint_iri": sprint_iri,
                    "sprint_order": order,
                    "criteria_doc": criteria,
                },
                "findings": findings,
                "error": "unknown finding severity blocks dispatch",
            }, indent=2))
            raise SystemExit(1)

        print(json.dumps({
            "phase": "TRAVERSAL",
            "vars": {
                "sprint": sprint_local,
                "sprint_iri": sprint_iri,
                "sprint_order": order,
                "criteria_doc": criteria,
            },
            "findings": findings,
        }, indent=2))
        return

    # ── Cursor empty — verify all sprints have valid Completions ─────────────
    incomplete_rows = _cli_run_sparql(
        g, script_dir / "all-complete.sparql", {"PHASE": phase_iri}
    )
    if incomplete_rows:
        # Some sprints are in-flight or have invalid Completions
        print(json.dumps({
            "phase": "AWAITING",
            "vars": {},
            "_incomplete_sprints": [str(r[0]) for r in incomplete_rows],
        }, indent=2))
        return

    # ── Check for open non-blocking findings (CLEANUP) ───────────────────────
    cleanup_rows = _cli_run_sparql(
        g, script_dir / "open-findings-sprint.sparql", {"PHASE": phase_iri}
    )

    if cleanup_rows:
        findings = [
            {
                "sprint_iri": str(r[1]),
                "severity": str(r[2]),
                "foundAt": str(r[3]),
                "description": str(r[4]),
            }
            for r in cleanup_rows
        ]
        print(json.dumps({
            "phase": "CLEANUP",
            "vars": {},
            "_findings_raw": findings,
        }, indent=2))
    else:
        print(json.dumps({
            "phase": "DONE",
            "vars": {},
            "_findings_raw": [],
        }, indent=2))


if __name__ == "__main__":
    main()
