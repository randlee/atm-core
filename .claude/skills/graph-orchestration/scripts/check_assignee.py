#!/usr/bin/env python3
"""
check_assignee.py — Is a given assignee already busy in this phase?

Called by assignee-busy. Returns JSON to stdout; errors to stderr.

Team-lead must run this (via the assignee-busy wrapper) and confirm
"busy": false before appending any new Assignment event for an agent,
regardless of what cursor.sparql / next-dev-task returns. Cursor readiness
and assignee availability are two independent checks — a sprint being
"ready" never overrides an agent already having open work.

Output shape:
  {
    "busy": true,
    "assignee": "arch-ctm",
    "open_assignments": [
      {"sprint": "urn:atm:triage:AICH-S2", "assignedAt": "2026-07-25T04:45:03Z"}
    ]
  }

Usage: check_assignee.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> <ASSIGNEE>
"""

import sys
import json
from pathlib import Path

from rdflib import URIRef, Literal

from query_runner import load_graph, run_sparql, TRIAGE_BASE


def main() -> int:
    if len(sys.argv) != 5:
        print(
            "Usage: check_assignee.py <PHASE_LOCAL> <TTL_DIR> <SCRIPT_DIR> <ASSIGNEE>",
            file=sys.stderr,
        )
        return 1

    phase_local = sys.argv[1]
    ttl_dir = sys.argv[2]
    script_dir = Path(sys.argv[3])
    assignee = sys.argv[4]

    phase_iri = URIRef(f"{TRIAGE_BASE}Phase{phase_local}")
    try:
        g = load_graph(ttl_dir)
        rows = run_sparql(
            g,
            script_dir / "assignee-busy.sparql",
            {"PHASE": phase_iri, "ASSIGNEE": Literal(assignee)},
        )
    except Exception as exc:  # noqa: BLE001 - CLI boundary must be one-line
        print(f"ERROR: assignee check failed: {exc}", file=sys.stderr)
        return 1

    print(json.dumps({
        "busy": bool(rows),
        "assignee": assignee,
        "open_assignments": [
            {"sprint": str(r[0]), "assignedAt": str(r[1])} for r in rows
        ],
    }, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
