#!/usr/bin/env python3
"""Read-only, injectable pre-claim gates for dev, sanity, and QA work, and the read-only stack-top lookup."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any, Callable

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "atm-beads/scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from stack_top import select_stack_top, stack_bead_id

Runner = Callable[..., subprocess.CompletedProcess[str]]
IGNORED_STATUS_PATHS = (".beads.gate.lock", ".sc-compose/")
PRIMARY = Path(__file__).resolve().parents[4]
VALIDATE_PLAN = str(PRIMARY / ".claude/skills/atm-beads/scripts/validate-plan")
COMMAND_TIMEOUT = 30


def invoke(runner: Runner, argv: list[str], **kwargs: Any) -> subprocess.CompletedProcess[str]:
    try:
        return runner(argv, timeout=COMMAND_TIMEOUT, **kwargs)
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"{argv!r} timed out after {COMMAND_TIMEOUT}s") from error


def run(runner: Runner, *args: str, cwd: str | None = None) -> str:
    options = {"cwd": cwd} if cwd is not None else {}
    result = invoke(runner, list(args), capture_output=True, text=True, **options)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "command failed")
    return result.stdout.strip()


def run_json(runner: Runner, *args: str) -> Any:
    return json.loads(run(runner, *args))


def metadata(bead: dict[str, Any]) -> dict[str, Any]:
    value = bead.get("metadata")
    return value if isinstance(value, dict) else {}


def claimable(bead: dict[str, Any], identity: str) -> bool:
    return bead.get("status") == "open" and (not bead.get("assignee") or bead.get("assignee") == identity)


def has_prior_pass(output: str) -> bool:
    """A `bd history --json` snapshot of the bead closed with reason `PASS at <sha>`; a note or reason that merely
    mentions PASS is not one. Output that is not a JSON list of snapshots, each with an `Issue` object, raises."""
    try:
        history = json.loads(output)
    except ValueError:
        history = None
    if not isinstance(history, list) or not all(isinstance(row, dict) and isinstance(row.get("Issue"), dict) for row in history):
        raise ValueError(f"bd history --json is not a JSON list of snapshots with an Issue object: {output}")
    return any(row["Issue"].get("status") == "closed"
               and str(row["Issue"].get("close_reason") or "").startswith("PASS at ") for row in history)


def is_clean(status: str) -> bool:
    return not any(line and not any(path in line for path in IGNORED_STATUS_PATHS) for line in status.splitlines())


def git_dir(args: argparse.Namespace) -> list[str]:
    worktree = getattr(args, "worktree", "")
    return ["-C", worktree] if worktree else []


def declared_pr_target(runner: Runner, bead: dict[str, Any]) -> str | None:
    """The bead's own pr_target (planned: its nearest `must_follow` prerequisite's branch, or the trunk; never a
    parallel sibling); a poured dev bead declares none and takes its sprint container's.
    A fix bead's target (a new layer at the top of the stack) is set at dispatch."""
    target = metadata(bead).get("pr_target")
    container = metadata(bead).get("sprint_bead")
    if target is None and container and "stage:dev" in (bead.get("labels") or []):
        target = metadata(run_json(runner, "bd", "show", str(container), "--json")[0]).get("pr_target")
    return target


def descends(runner: Runner, lower: str, base: str, git: list[str] | None = None) -> bool:
    """`pr_target` is a lower bound: the actual base is it or a descendant of it.
    A lower bound whose branch is gone from origin (its layer collapsed into the trunk) holds when a merged PR of it
    has its merge commit in `base`."""
    if lower == base:
        return True
    result = invoke(runner, ["git", *(git or []), "merge-base", "--is-ancestor", f"origin/{lower}", f"origin/{base}"], capture_output=True, text=True)
    if result.returncode > 1:
        if invoke(runner, ["git", *(git or []), "rev-parse", "--verify", "--quiet", f"refs/remotes/origin/{lower}"], capture_output=True, text=True).returncode:
            for pr in run_json(runner, "gh", "pr", "list", "--head", lower, "--state", "merged", "--json", "mergeCommit"):
                if invoke(runner, ["git", *(git or []), "merge-base", "--is-ancestor", str((pr.get("mergeCommit") or {}).get("oid")), f"origin/{base}"],
                          capture_output=True, text=True).returncode == 0:
                    return True
        raise RuntimeError(result.stderr.strip() or "git merge-base failed")
    return result.returncode == 0


def stack_top(runner: Runner, args: argparse.Namespace) -> str:
    rows = json.loads(run(runner, "bd", "show", stack_bead_id(args.root), "--json", cwd=str(PRIMARY)))
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise ValueError("stack bead lookup must return exactly one bead object")
    branch, _head = select_stack_top(
        rows[0], args.pr_target,
        lambda target: run(runner, "git", *git_dir(args), "rev-parse", f"origin/{target}"),
    )
    return branch


def dev_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    plan = invoke(runner, [VALIDATE_PLAN, "--root", args.root], capture_output=True, text=True, cwd=str(PRIMARY))
    if plan.returncode == 5:
        print((plan.stdout + plan.stderr).strip(), file=sys.stderr)
        return "PLAN_INVALID"
    if plan.returncode:
        raise RuntimeError(f"validate-plan exit {plan.returncode}: {(plan.stderr or plan.stdout).strip()}")
    ready = run_json(runner, "bd", "ready", "-n", "0", "--json")
    if not any(row.get("id") == args.bead for row in ready):
        return "NOT_READY"
    bead = run_json(runner, "bd", "show", args.bead, "--json")[0]
    if not claimable(bead, identity):
        return "UNCLAIMABLE"
    declared = declared_pr_target(runner, bead)
    if declared is not None and not descends(runner, str(declared), args.pr_target, git_dir(args)):
        return "PR_TARGET_MISMATCH"
    if invoke(runner, ["git", *git_dir(args), "merge-base", "--is-ancestor", f"origin/{args.pr_target}", "HEAD"], capture_output=True, text=True).returncode:
        return "WRONG_BASE"
    return "READY"


def sanity_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if not args.commit:
        return "COMMIT_REQUIRED"
    if has_prior_pass(run(runner, "bd", "history", args.bead, "--json")):
        return "SANITY_FROZEN"
    if run(runner, "git", *git_dir(args), "rev-parse", "HEAD") != args.commit:
        return "COMMIT_MISMATCH"
    return "READY"


def sanity_pass_commit(runner: Runner, checked: str) -> str | None:
    """The sha in the close reason `PASS at <sha>` of the latest passed sanity check of `checked`."""
    rows = run_json(runner, "bd", "list", "-l", "stage:dev-sanity", "--status", "closed", "-n", "0", "--json")
    passes = [row for row in rows if metadata(row).get("dev_bead") == checked
              and str(row.get("close_reason") or "").startswith("PASS at ")]
    if not passes:
        return None
    reason = str(max(passes, key=lambda row: str(row.get("closed_at") or ""))["close_reason"])
    return (reason[len("PASS at "):].split() or [""])[0].rstrip(".:;,")


def qa_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if not args.pr_number:
        return "PR_REQUIRED"
    qa_bead = run_json(runner, "bd", "show", args.bead, "--json")[0]
    checked = args.checked_bead or metadata(qa_bead).get("checked_bead")
    pass_commit = sanity_pass_commit(runner, str(checked)) if checked else None
    pr = run_json(runner, "gh", "pr", "view", args.pr_number, "--json", "baseRefName,headRefOid")
    base = str(pr.get("baseRefName") or "")
    if not all(descends(runner, str(lower), base) for lower in (metadata(qa_bead).get("pr_target"), args.pr_target) if lower is not None):
        return "PR_TARGET_MISMATCH"
    quick_fix = metadata(qa_bead).get("quick_fix") is True  # a Parallel Quick Fix has QA but no sanity check
    if not quick_fix and (not pass_commit or len(pass_commit) < 7 or not str(pr.get("headRefOid") or "").startswith(pass_commit)):
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
    except (RuntimeError, OSError, IndexError, TypeError, ValueError, KeyError) as exc:
        print(f"assignment-gates: {exc}", file=sys.stderr)
        return "GATE_CANNOT_RUN"


def main(argv: list[str] | None = None, runner: Runner = subprocess.run) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("kind", choices=("dev", "sanity", "qa", "stack-top"))
    parser.add_argument("--root", default="")
    parser.add_argument("--bead", default="")
    parser.add_argument("--pr-target", default="")
    parser.add_argument("--identity", default="")
    parser.add_argument("--pr-number", default="")
    parser.add_argument("--commit", default="")
    parser.add_argument("--checked-bead", default="")
    parser.add_argument("--worktree", default="")
    parser.add_argument("--base", default="", help="accepted for compatibility; sanity does not check PR bases")
    args = parser.parse_args(argv)
    if args.kind != "sanity" and not args.pr_target:
        parser.error(f"{args.kind} needs --pr-target")
    if args.kind == "stack-top":
        if not args.root:
            parser.error("stack-top needs --root")
        try:
            top = stack_top(runner, args)
        except (RuntimeError, OSError, TypeError, ValueError, KeyError) as exc:
            print(f"assignment-gates: {exc}", file=sys.stderr)
            print("GATE_CANNOT_RUN")
            return 2
        print(top)
        return 0
    if args.kind == "sanity" and not args.bead:
        parser.error("sanity needs --bead")
    if args.kind != "sanity" and not (args.root and args.bead):
        parser.error(f"{args.kind} needs --root and --bead")
    code = evaluate(args, runner)
    print(code)
    return 0 if code == "READY" else (2 if code == "GATE_CANNOT_RUN" else 5)


if __name__ == "__main__":
    raise SystemExit(main())
