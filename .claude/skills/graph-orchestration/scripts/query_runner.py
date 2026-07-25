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
    from rdflib.namespace import XSD
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
    # Load triage findings from repo root .triage/ (relative to TTL dir's parent)
    # Walk up from ttl_dir to find repo root (contains .triage/)
    repo_root = _find_repo_root(base)
    if repo_root:
        for findings_file in sorted(repo_root.glob(".triage/*/findings/*.ttl")):
            g.parse(str(findings_file), format="turtle")
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
