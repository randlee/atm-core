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
triage-findings skill to decide between dev-task.xml.j2 and dev-fix.xml.j2.
Assignments are appended to events.ttl by the orchestrator after dispatch.

Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR>
  PHASE_LOCAL  e.g. "F"
  TTL_DIR      path to .sprints/<PHASE>/ directory on integrate/phase-N branch
  SCRIPT_DIR   path to this script's directory (for .sparql files)
"""

import sys
import json
from pathlib import Path

try:
    from rdflib import Graph, URIRef, Namespace
    from rdflib.namespace import RDF, XSD
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


def load_graph(ttl_dir: str) -> Graph:
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
    repo_root = _find_repo_root(base)
    if repo_root:
        for findings_file in sorted(repo_root.glob(".triage/*/findings/*.ttl")):
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


def main():
    if len(sys.argv) != 4:
        print("Usage: query_runner.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR>", file=sys.stderr)
        sys.exit(1)

    phase_local = sys.argv[1]
    ttl_dir = sys.argv[2]
    script_dir = Path(sys.argv[3])

    phase_iri = URIRef(f"{TRIAGE_BASE}Phase{phase_local}")
    g = load_graph(ttl_dir)

    # ── Validate structure before cursor ─────────────────────────────────────
    validate_rows = run_sparql(g, script_dir / "validate-structure.sparql", {"PHASE": phase_iri})
    if validate_rows:
        for row in validate_rows:
            print(f"ERROR: structure violation: {row[0]} — {row[1]}", file=sys.stderr)
        sys.exit(1)

    # ── Cursor ───────────────────────────────────────────────────────────────
    cursor_rows = run_sparql(g, script_dir / "cursor.sparql", {"PHASE": phase_iri})

    if cursor_rows:
        sprint_iri = str(cursor_rows[0][0])
        order = int(cursor_rows[0][1])
        criteria = str(cursor_rows[0][2])
        sprint_local = sprint_iri.split(":")[-1] if ":" in sprint_iri else sprint_iri

        print(json.dumps({
            "phase": "TRAVERSAL",
            "vars": {
                "sprint": sprint_local,
                "sprint_iri": sprint_iri,
                "sprint_order": order,
                "criteria_doc": criteria,
            },
        }, indent=2))
        return

    # ── Cursor empty — verify all sprints have valid Completions ─────────────
    incomplete_rows = run_sparql(g, script_dir / "all-complete.sparql", {"PHASE": phase_iri})
    if incomplete_rows:
        # Some sprints are in-flight or have invalid Completions
        print(json.dumps({
            "phase": "AWAITING",
            "vars": {},
            "_incomplete_sprints": [str(r[0]) for r in incomplete_rows],
        }, indent=2))
        return

    # ── Check for open non-blocking findings (CLEANUP) ───────────────────────
    cleanup_rows = run_sparql(
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
