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
from pathlib import Path

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF
except ImportError:
    print("ERROR: rdflib not installed. Run: pip3 install rdflib", file=sys.stderr)
    sys.exit(1)

TRIAGE_BASE = "urn:atm:triage:"
TRIAGE = Namespace(TRIAGE_BASE)


def _find_repo_root(start: Path):
    """Walk up from start to find the directory containing .triage/"""
    current = start.resolve()
    for _ in range(10):  # max 10 levels up
        if (current / ".triage").exists():
            return current
        parent = current.parent
        if parent == current:
            break
        current = parent
    return None


IGNORE_FILE_NAME = ".graph-orchestration-ignore"


def _load_ignored_phase_dirs(repo_root: Path) -> set:
    """Read repo_root/.triage/.graph-orchestration-ignore into a set of names.

    This is a purely optional, best-effort efficiency/noise-reduction layer:
    it lets known-dead legacy phase directories (whose findings files use an
    old pre-Turtle format and will never be migrated) be skipped entirely
    before ever attempting to open or parse a file under them, avoiding both
    wasted I/O and a repeated stderr `WARNING: skipping malformed findings
    file ...` on every single invocation.

    It is NOT a correctness mechanism and must never be treated as one: the
    membership-based scoping in `load_graph()` (via `known_sprints` /
    `triage:foundIn`) is what actually prevents cross-phase contamination,
    for *every* findings directory, including ones not listed here. Absence
    of a directory from this ignore list — or absence of the file itself —
    changes nothing about correctness, only about how much redundant parsing
    and warning noise is produced for directories known in advance to be
    dead.
    """
    ignore_path = repo_root / ".triage" / IGNORE_FILE_NAME
    if not ignore_path.exists():
        return set()

    names = set()
    for line in ignore_path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        names.add(line)
    return names


def load_graph(ttl_dir: str, *, include_findings: bool = True) -> Graph:
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

    # Collect the set of triage:Sprint subjects declared by *this phase's*
    # structure.ttl (+ events.ttl, though sprints are only ever declared in
    # structure.ttl in practice). This is the authoritative membership set
    # used below to scope findings — it is derived from the graph itself,
    # not from directory names or naming conventions, so it can't be
    # defeated by a future phase reusing an unprefixed or colliding local
    # sprint label.
    known_sprints = set(g.subjects(RDF.type, TRIAGE.Sprint))

    if not include_findings:
        return g

    # Load triage findings from repo root .triage/ (relative to TTL dir's parent)
    # Walk up from ttl_dir to find repo root (contains .triage/)
    #
    # Findings are filed under `.triage/<phase_id>/findings/*.ttl`, where
    # `<phase_id>` (e.g. "phase-AI") is a project-phase directory that does
    # not necessarily match the `PHASE_LOCAL` sprint-batch label (e.g.
    # "AICH") passed to this script — there is no discoverable
    # findings-path-to-phase mapping in structure.ttl today (no
    # `triage:findingsPath` or similar property), so we cannot statically
    # narrow the *glob* to "the one right directory" for a given phase.
    #
    # Directory-name matching was considered and rejected as the scoping
    # mechanism: it's convention-dependent (relies on `phase-AI` matching
    # `AICH`-prefixed sprint labels) and nothing enforces it — a future
    # phase could reuse an unprefixed or colliding local sprint label with
    # no directory-name collision to catch it, silently attaching a stray
    # well-formed finding to the wrong phase's sprint and corrupting cursor
    # resolution.
    #
    # Instead, each findings file is parsed into its own temporary Graph
    # first (still isolated in try/except, see below), and only the triples
    # belonging to findings whose `triage:foundIn` object is in
    # `known_sprints` (this phase's *actual* declared triage:Sprint
    # subjects, read directly from structure.ttl/events.ttl above) are
    # merged into `g`. This is real membership scoping, not a naming
    # convention: it is enforced by graph structure, so it cannot be
    # defeated by directory or label collisions. Findings whose
    # `triage:foundIn` points at a sprint outside this set are dropped
    # silently — they're simply out of scope for this phase, not an error.
    #
    # A single malformed (non-Turtle) findings file anywhere in the repo
    # must also not abort the entire parse and crash cursor resolution for
    # every phase, including ones with no relationship to the offending
    # file — hence the per-file try/except below, which remains necessary
    # defense-in-depth independent of the scoping filter.
    #
    # Some `.triage/<phase_id>/` directories are permanently closed/dead
    # legacy phases whose findings files were written in an old pre-Turtle
    # format and will never be migrated. Repeatedly globbing, opening, and
    # failing to parse those files on every single invocation is pure,
    # predictable waste: the outcome never changes. `.triage/.graph-
    # orchestration-ignore` (see `_load_ignored_phase_dirs`) lets such
    # directories be named explicitly so they're skipped before `.parse()`
    # is ever called on them — no warning is printed for these, since the
    # directory was deliberately and explicitly acknowledged as dead, unlike
    # an unexpected malformed file that wasn't. This is purely an efficiency
    # optimization layered on top of the membership-based scoping above; it
    # does not replace it, and directories absent from the ignore list are
    # still fully protected by the `known_sprints` membership filter.
    repo_root = _find_repo_root(base)
    if repo_root:
        ignored_phase_dirs = _load_ignored_phase_dirs(repo_root)
        for findings_file in sorted(repo_root.glob(".triage/*/findings/*.ttl")):
            phase_dir_name = findings_file.parent.parent.name
            if phase_dir_name in ignored_phase_dirs:
                continue
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
                for triple in file_graph.triples((finding, None, None)):
                    g.add(triple)
                for resolution in file_graph.subjects(TRIAGE.resolves, finding):
                    for triple in file_graph.triples((resolution, None, None)):
                        g.add(triple)
    return g


def run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    results = g.query(sparql_file.read_text(), initBindings=bindings)
    return list(results)


def _cli_load_graph(ttl_dir: str, *, include_findings: bool) -> Graph:
    """Load a graph while keeping malformed input errors CLI-friendly."""
    try:
        return load_graph(ttl_dir, include_findings=include_findings)
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: query runner failed to load graph: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


def _cli_run_sparql(g: Graph, sparql_file: Path, bindings: dict) -> list:
    """Run one bundled query while keeping query errors one-line."""
    try:
        return run_sparql(g, sparql_file, bindings)
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: query runner failed to run {sparql_file}: {exc}", file=sys.stderr)
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


def _validate_findings_before_query(ttl_dir: str, script_dir: Path) -> None:
    """Run raw findings validation before *any* graph query.

    ``load_graph`` intentionally scopes findings by ``foundIn``.  Running this
    gate first prevents malformed or incomplete records from disappearing in
    that scope filter.  A normal validation failure (exit-1 equivalent) and a
    validator execution error both stop cursor resolution; warnings alone are
    allowed by the validator's discriminated result contract.
    """

    repo_root = _find_repo_root(Path(ttl_dir))
    if repo_root is None:
        raise RuntimeError(
            "cannot locate repository root containing .triage; findings validation cannot run"
        )
    triage_root = repo_root / ".triage"
    findings_dirs = sorted(
        path for path in triage_root.glob("*/findings") if path.is_dir()
    ) if triage_root.exists() else []
    # An existing but empty .triage tree is a valid no-findings input.  Passing
    # the root directory still exercises the validator and gives a structured
    # error if the directory itself is missing or unreadable.
    if not findings_dirs:
        findings_dirs = [triage_root]

    validator = _load_validator(script_dir)
    structure = Path(ttl_dir) / "structure.ttl"
    events_path = Path(ttl_dir) / "events.ttl"
    events = events_path if events_path.exists() else None

    for findings_dir in findings_dirs:
        result = validator.run_validation(
            findings_dir=findings_dir,
            structure=structure,
            events=events,
            script_dir=script_dir,
        )
        if result.kind == "validation:pass":
            continue
        summary = getattr(result, "summary", None)
        counts = (
            f" ({summary.errors} error(s), {summary.warnings} warning(s))"
            if summary is not None
            else ""
        )
        print(
            f"ERROR: findings validation blocked query resolution for {findings_dir}"
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
            raise SystemExit(2)
        raise SystemExit(1)


def main():
    if len(sys.argv) not in (4, 5) or (
        len(sys.argv) == 5 and sys.argv[4] != "--validate-only"
    ):
        print(
            "Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> "
            "[--validate-only]",
            file=sys.stderr,
        )
        sys.exit(1)

    phase_local = sys.argv[1]
    ttl_dir = sys.argv[2]
    script_dir = Path(sys.argv[3])
    validate_only = len(sys.argv) == 5

    phase_iri = URIRef(f"{TRIAGE_BASE}Phase{phase_local}")
    # This is deliberately before structure loading and before --validate-only:
    # every query_runner entry point must prove that raw findings are valid.
    try:
        _validate_findings_before_query(ttl_dir, script_dir)
    except SystemExit:
        raise
    except Exception as exc:  # noqa: BLE001 - CLI boundary
        print(f"ERROR: findings validation could not run: {exc}", file=sys.stderr)
        raise SystemExit(2) from exc

    # The raw findings gate above runs before structure validation so malformed
    # or incomplete records cannot disappear during phase membership filtering.
    # Once that gate passes, structure validation can report graph-shape errors
    # without being conflated with finding-schema diagnostics.
    g = _cli_load_graph(ttl_dir, include_findings=not validate_only)

    # ── Validate structure before cursor ─────────────────────────────────────
    validate_rows = _cli_run_sparql(
        g, script_dir / "validate-structure.sparql", {"PHASE": phase_iri}
    )
    if validate_rows:
        for row in validate_rows:
            print(f"ERROR: structure violation: {row[0]} — {row[1]}", file=sys.stderr)
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
