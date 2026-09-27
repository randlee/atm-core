#!/usr/bin/env python3
"""Pre-claim refusal gates used by dev, sanity, and QA assignments.

The command is deliberately advisory: it never force-claims or edits a work
bead.  A caller that receives a refusal code creates the workflow-issue bead
and closes/refuses its ATM task through the normal template.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
from plan_contract import model_matches  # noqa: E402


def run_json(*args: str) -> Any:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "command failed")
    return json.loads(result.stdout)


def metadata(bead: dict[str, Any]) -> dict[str, Any]:
    value = bead.get("metadata")
    return value if isinstance(value, dict) else {}


def refusal_for_difficulty(bead: dict[str, Any], members: list[dict[str, Any]], identity: str) -> str | None:
    difficulty = metadata(bead).get("difficulty")
    member = next((row for row in members if row.get("identity") == identity or row.get("id") == identity), None)
    model = (member or {}).get("model")
    if not isinstance(difficulty, str) or not model_matches(str(model or ""), difficulty):
        return "DIFFICULTY_MISMATCH"
    return None


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("dev", "sanity", "qa"))
    parser.add_argument("--root", required=True)
    parser.add_argument("--bead", required=True)
    parser.add_argument("--pr-target", required=True)
    parser.add_argument("--identity", default="")
    args = parser.parse_args()
    try:
        validate = subprocess.run([
            ".claude/skills/atm-beads/scripts/validate-plan", "--root", args.root, "--scope", args.bead
        ], capture_output=True, text=True)
        if validate.returncode:
            print("PLAN_INVALID")
            return 5
        ready = run_json("bd", "ready", "-n", "0", "--json")
        if not any(row.get("id") == args.bead for row in ready):
            print("NOT_READY")
            return 5
        bead_rows = run_json("bd", "show", args.bead, "--json")
        bead = bead_rows[0]
        if metadata(bead).get("pr_target") not in (None, args.pr_target):
            print("PR_TARGET_MISMATCH")
            return 5
        identity = args.identity or __import__("os").environ.get("ATM_IDENTITY", "")
        members = run_json("atm", "members", "--json")
        if not isinstance(members, list):
            members = members.get("members", [])
        reason = refusal_for_difficulty(bead, members, identity)
        if reason:
            print(reason)
            return 5
        base = subprocess.run(["git", "merge-base", "--is-ancestor", f"origin/{args.pr_target}", "HEAD"])
        if base.returncode:
            print("WRONG_BASE")
            return 5
    except (RuntimeError, OSError, IndexError, TypeError, ValueError) as exc:
        print(f"GATE_CANNOT_RUN: {exc}")
        return 2
    print("READY")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
