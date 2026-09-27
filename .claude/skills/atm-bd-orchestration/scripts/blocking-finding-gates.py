#!/usr/bin/env python3
"""Create R16 downstream gates for a newly filed blocking finding."""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads" / "scripts"))
from plan_contract import FINDING_LABEL, FIX_LABEL, SANITY_LABEL


def deps(bead: dict[str, Any]) -> set[str]:
    return {str(edge.get("depends_on_id") or edge.get("id")) for edge in bead.get("dependencies") or []
            if str(edge.get("type") or edge.get("dependency_type")) == "blocks"}


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
    source_gate = next((str(bead.get("id")) for bead in all_beads
                        if SANITY_LABEL in (bead.get("labels") or [])
                        and str((bead.get("metadata") or {}).get("dev_bead")) == source), None)
    if not source_gate: raise RuntimeError("source sprint has no sanity bead")
    downstream = {str(bead.get("id")) for bead in all_beads if "stage:dev" in (bead.get("labels") or [])
                  and bead.get("status") == "open" and source_gate in closure(str(bead.get("id")), by_id)}
    result = set(downstream)
    for bead in all_beads:
        meta = bead.get("metadata") or {}
        if (bead.get("status") == "open" and not bead.get("assignee")
                and ({FINDING_LABEL, FIX_LABEL} & set(bead.get("labels") or []))
                and meta.get("sprint_bead") in downstream):
            result.add(str(bead.get("id")))
    return source_gate, sorted(result)


def cmd(*args: str, capture: bool = False) -> str:
    run = subprocess.run(args, text=True, capture_output=capture)
    if run.returncode: raise RuntimeError(run.stderr.strip() or run.stdout.strip())
    return run.stdout.strip()


def main() -> int:
    parser = argparse.ArgumentParser(); parser.add_argument("--finding", required=True); args = parser.parse_args()
    try:
        finding = json.loads(cmd("bd", "show", args.finding, "--json", capture=True))[0]
        all_beads = json.loads(cmd("bd", "list", "--all", "-n", "0", "--json", capture=True))
        source_gate, selected = targets(finding, all_beads)
        if source_gate is None:
            print(json.dumps({"finding": args.finding, "edges": []})); return 0
        gate = f"{args.finding}-sanity"
        existing = {str(bead.get("id")) for bead in all_beads}
        if gate not in existing:
            cmd("bd", "create", "--id", gate, "--type", "task", "--status", "open", "--parent", args.finding,
                "--title", f"sanity gate for {args.finding}", "--labels", SANITY_LABEL,
                "--metadata", json.dumps({"dev_bead": args.finding}), "--silent")
        by_id = {str(bead.get("id")): bead for bead in all_beads}
        for target in selected:
            if gate not in deps(by_id[target]):
                cmd("bd", "dep", "add", target, gate, "--type", "blocks")
        print(json.dumps({"finding": args.finding, "gate": gate, "edges": selected}))
        return 0
    except (RuntimeError, ValueError, IndexError) as exc:
        print(f"BLOCKING_GATE_FAILED: {exc}", file=sys.stderr); return 2


if __name__ == "__main__": raise SystemExit(main())
