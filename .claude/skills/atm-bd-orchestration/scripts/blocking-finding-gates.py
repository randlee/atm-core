#!/usr/bin/env python3
"""Create R16 downstream gates for a newly filed blocking finding."""
from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any

from bd_commands import (
    BLOCKS_RELATION,
    DEV_LABEL,
    FINDING_LABEL,
    FIX_LABEL,
    SANITY_LABEL,
    BdCommandError,
    command,
)


def deps(bead: dict[str, Any]) -> set[str]:
    return {str(edge.get("depends_on_id") or edge.get("id")) for edge in bead.get("dependencies") or []
            if str(edge.get("type") or edge.get("dependency_type")) == BLOCKS_RELATION}


def closure(bead_id: str, all_beads: dict[str, dict[str, Any]]) -> set[str]:
    seen: set[str] = set(); pending = [bead_id]
    while pending:
        current = pending.pop()
        for parent in deps(all_beads.get(current, {})) - seen:
            seen.add(parent); pending.append(parent)
    return seen


def targets(finding: dict[str, Any], all_beads: list[dict[str, Any]]) -> tuple[str | None, list[str]]:
    meta = finding.get("metadata") or {}
    if meta.get("severity") != "blocking": return None, []
    source = meta.get("sprint_bead")
    by_id = {str(bead.get("id")): bead for bead in all_beads}
    by_id.setdefault(str(finding.get("id")), finding)
    source_gate = next((str(bead.get("id")) for bead in all_beads
                        if SANITY_LABEL in (bead.get("labels") or [])
                        and str((bead.get("metadata") or {}).get("dev_bead")) == source), None)
    if not source_gate: raise RuntimeError("source sprint has no sanity bead")
    exempt = closure(str(finding.get("id")), by_id)  # a gate cannot block what its finding waits on
    downstream = {str(bead.get("id")) for bead in all_beads if DEV_LABEL in (bead.get("labels") or [])
                  and bead.get("status") == "open" and source_gate in closure(str(bead.get("id")), by_id)} - exempt
    result = set(downstream)
    for bead in all_beads:
        meta = bead.get("metadata") or {}
        if (bead.get("status") == "open" and not bead.get("assignee") and str(bead.get("id")) not in exempt
                and ({FINDING_LABEL, FIX_LABEL} & set(bead.get("labels") or []))
                and meta.get("sprint_bead") in downstream):
            result.add(str(bead.get("id")))
    return source_gate, sorted(result)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--finding", required=True)
    parser.add_argument("--actor", default=os.environ.get("ATM_IDENTITY", ""))
    args = parser.parse_args()
    try:
        finding = json.loads(command(["bd", "show", args.finding, "--json"], args.actor, capture=True))[0]
        all_beads = json.loads(command(["bd", "list", "--all", "-n", "0", "--json"], args.actor, capture=True))
        source_gate, selected = targets(finding, all_beads)
        if source_gate is None:
            print(json.dumps({"finding": args.finding, "edges": []})); return 0
        sprint_bead = str((finding.get("metadata") or {}).get("sprint_bead") or "")
        if not sprint_bead:
            raise RuntimeError("blocking finding has no sprint_bead metadata")
        gate = f"{args.finding}-sanity"
        by_id = {str(bead.get("id")): bead for bead in all_beads}
        existing = set(by_id)
        if gate not in existing:
            command(["bd", "create", "--id", gate, "--type", "task", "--status", "open",
                     "--title", f"sanity gate for {args.finding}", "--labels", SANITY_LABEL,
                     "--metadata", json.dumps({"dev_bead": args.finding}), "--silent"], args.actor)
            command(["bd", "update", gate, "--parent", sprint_bead], args.actor)
            by_id[gate] = {"id": gate, "dependencies": []}
        if args.finding not in deps(by_id[gate]):
            command(["bd", "dep", "add", gate, args.finding, "--type", BLOCKS_RELATION], args.actor)
        for target in selected:
            if gate not in deps(by_id[target]):
                command(["bd", "dep", "add", target, gate, "--type", BLOCKS_RELATION], args.actor)
        print(json.dumps({"finding": args.finding, "gate": gate, "edges": selected}))
        return 0
    except (BdCommandError, RuntimeError, ValueError, IndexError) as exc:
        print(f"BLOCKING_GATE_FAILED: {exc}", file=sys.stderr); return 2


if __name__ == "__main__": raise SystemExit(main())
