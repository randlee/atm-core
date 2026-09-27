#!/usr/bin/env python3
"""Read-only, injectable pre-claim gates for dev, sanity, and QA work."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
from plan_contract import model_matches  # noqa: E402

Runner = Callable[..., subprocess.CompletedProcess[str]]
IGNORED_STATUS_PATHS = (".beads.gate.lock", ".sc-compose/")


def run(runner: Runner, *args: str) -> str:
    result = runner(list(args), capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "command failed")
    return result.stdout.strip()


def run_json(runner: Runner, *args: str) -> Any:
    return json.loads(run(runner, *args))


def metadata(bead: dict[str, Any]) -> dict[str, Any]:
    value = bead.get("metadata")
    return value if isinstance(value, dict) else {}


def refusal_for_difficulty(bead: dict[str, Any], members: list[dict[str, Any]], identity: str) -> str | None:
    difficulty = metadata(bead).get("difficulty")
    member = next((row for row in members if identity in (row.get("name"), row.get("identity"), row.get("id"))), None)
    if not isinstance(difficulty, str) or not model_matches(str((member or {}).get("model") or ""), difficulty):
        return "DIFFICULTY_MISMATCH"
    return None


def members_for(runner: Runner) -> list[dict[str, Any]]:
    members = run_json(runner, "atm", "members", "--json")
    return members if isinstance(members, list) else members.get("members", [])


def claimable(bead: dict[str, Any], identity: str) -> bool:
    return bead.get("status") == "open" and (not bead.get("assignee") or bead.get("assignee") == identity)


def has_prior_pass(history: Any) -> bool:
    return "PASS" in json.dumps(history).upper()


def is_clean(status: str) -> bool:
    return not any(line and not any(path in line for path in IGNORED_STATUS_PATHS) for line in status.splitlines())


def dev_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if runner([".claude/skills/atm-beads/scripts/validate-plan", "--root", args.root, "--scope", args.bead], capture_output=True, text=True).returncode:
        return "PLAN_INVALID"
    ready = run_json(runner, "bd", "ready", "-n", "0", "--json")
    if not any(row.get("id") == args.bead for row in ready):
        return "NOT_READY"
    bead = run_json(runner, "bd", "show", args.bead, "--json")[0]
    if not claimable(bead, identity):
        return "UNCLAIMABLE"
    if metadata(bead).get("pr_target") not in (None, args.pr_target):
        return "PR_TARGET_MISMATCH"
    if (reason := refusal_for_difficulty(bead, members_for(runner), identity)):
        return reason
    if runner(["git", "merge-base", "--is-ancestor", f"origin/{args.pr_target}", "HEAD"], capture_output=True, text=True).returncode:
        return "WRONG_BASE"
    return "READY"


def sanity_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if not args.pr_number or not args.commit:
        return "PR_REQUIRED"
    run_json(runner, "bd", "show", args.bead, "--json")
    pr = run_json(runner, "gh", "pr", "view", args.pr_number, "--json", "baseRefName,headRefOid")
    if pr.get("baseRefName") != args.pr_target or pr.get("headRefOid") != args.commit:
        return "STALE_BASE"
    run(runner, "git", "fetch", "origin")
    if run(runner, "git", "rev-parse", args.pr_target) != run(runner, "git", "rev-parse", f"origin/{args.pr_target}"):
        return "STALE_BASE"
    if not run(runner, "git", "log", "--format=%H", f"origin/{args.pr_target}..{args.commit}"):
        return "ZERO_DELTA"
    if not is_clean(run(runner, "git", "status", "--porcelain", "--untracked-files=no")):
        return "DIRTY_TREE"
    if has_prior_pass(run_json(runner, "bd", "history", args.bead, "--json")):
        return "SANITY_FROZEN"
    return "READY"


def qa_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if not args.pr_number:
        return "PR_REQUIRED"
    qa_bead = run_json(runner, "bd", "show", args.bead, "--json")[0]
    if metadata(qa_bead).get("pr_target") not in (None, args.pr_target):
        return "PR_TARGET_MISMATCH"
    checked = args.checked_bead or metadata(qa_bead).get("checked_bead")
    checked_bead = run_json(runner, "bd", "show", str(checked), "--json")[0] if checked else qa_bead
    pass_commit = metadata(checked_bead).get("sanity_pass_commit") or metadata(qa_bead).get("sanity_pass_commit")
    pr = run_json(runner, "gh", "pr", "view", args.pr_number, "--json", "baseRefName,headRefOid")
    if pr.get("baseRefName") != args.pr_target:
        return "PR_TARGET_MISMATCH"
    if not pass_commit or pr.get("headRefOid") != pass_commit:
        return "SANITY_STALE"
    if run(runner, "git", "rev-parse", "HEAD") != pr.get("headRefOid"):
        return "QA_HEAD_MISMATCH"
    return "READY"


def evaluate(args: argparse.Namespace, runner: Runner = subprocess.run) -> str:
    identity = args.identity or os.environ.get("ATM_IDENTITY", "")
    try:
        if args.kind == "dev":
            return dev_gate(args, runner, identity)
        if args.kind == "sanity":
            return sanity_gate(args, runner, identity)
        return qa_gate(args, runner, identity)
    except (RuntimeError, OSError, IndexError, TypeError, ValueError, KeyError):
        return "GATE_CANNOT_RUN"


def main(argv: list[str] | None = None, runner: Runner = subprocess.run) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("dev", "sanity", "qa"))
    parser.add_argument("--root", required=True)
    parser.add_argument("--bead", required=True)
    parser.add_argument("--pr-target", required=True)
    parser.add_argument("--identity", default="")
    parser.add_argument("--pr-number", default="")
    parser.add_argument("--commit", default="")
    parser.add_argument("--checked-bead", default="")
    args = parser.parse_args(argv)
    code = evaluate(args, runner)
    print(code)
    return 0 if code == "READY" else (2 if code == "GATE_CANNOT_RUN" else 5)


if __name__ == "__main__":
    raise SystemExit(main())
