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
from plan_contract import model_matches  # noqa: E402

Runner = Callable[..., subprocess.CompletedProcess[str]]
IGNORED_STATUS_PATHS = (".beads.gate.lock", ".sc-compose/")
PRIMARY = Path(__file__).resolve().parents[4]
VALIDATE_PLAN = str(PRIMARY / ".claude/skills/atm-beads/scripts/validate-plan")


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
    result = runner(["git", *(git or []), "merge-base", "--is-ancestor", f"origin/{lower}", f"origin/{base}"], capture_output=True, text=True)
    if result.returncode > 1:
        if runner(["git", *(git or []), "rev-parse", "--verify", "--quiet", f"refs/remotes/origin/{lower}"], capture_output=True, text=True).returncode:
            for pr in run_json(runner, "gh", "pr", "list", "--head", lower, "--state", "merged", "--json", "mergeCommit"):
                if runner(["git", *(git or []), "merge-base", "--is-ancestor", str((pr.get("mergeCommit") or {}).get("oid")), f"origin/{base}"],
                          capture_output=True, text=True).returncode == 0:
                    return True
        raise RuntimeError(result.stderr.strip() or "git merge-base failed")
    return result.returncode == 0


STACKS = ("gh", "api", "repos/{owner}/{repo}/stacks", "--paginate", "--jq", ".[]")


def open_stacks(runner: Runner) -> list[dict[str, Any]]:
    """Every open stack from GitHub's stacks API, PRs bottom first; never local gh-stack tracking."""
    result = runner(list(STACKS), capture_output=True, text=True, cwd=str(PRIMARY))
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or "gh api stacks failed")
    return [stack for stack in (json.loads(line) for line in result.stdout.splitlines() if line.strip()) if stack.get("open")]


def is_open(pr: dict[str, Any]) -> bool:
    return pr.get("state") == "open" and not pr.get("merged_at")


def open_pr_heads(runner: Runner, flag: str, ref: str) -> list[str]:
    """Head branches of the open PRs whose `--base` or `--head` is `ref`, linked into a stack or not."""
    rows = json.loads(run_in(runner, str(PRIMARY), "gh", "pr", "list", flag, ref, "--state", "open", "--json", "headRefName"))
    return [str(row["headRefName"]) for row in rows]


def run_in(runner: Runner, cwd: str, *args: str) -> str:
    result = runner(list(args), capture_output=True, text=True, cwd=cwd)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip() or "command failed")
    return result.stdout


def stack_top(runner: Runner, target: str) -> str | None:
    """The branch a new PR based on `target` lands on: the head of the last open PR of the one open stack whose base is
    `target` or that has a PR from `target` (its base when none is open). When no stack matches, the top of the one
    chain of open unlinked PRs based on `target` (layer 0 awaiting layer 1: a stack forms from layers 0 and 1
    together, and layer 1 may open before the lead links them), else `target`. None when several stacks or unlinked
    PRs match at any step (ambiguous: never guess)."""
    matches = [stack for stack in open_stacks(runner)
               if stack["base"]["ref"] == target or any(pr["head"]["ref"] == target for pr in stack["pull_requests"])]
    if len(matches) > 1:
        return None
    if not matches:
        top, seen = target, {target}
        while (unlinked := open_pr_heads(runner, "--base", top)):
            if len(unlinked) > 1 or unlinked[0] in seen:
                return None
            top = unlinked[0]
            seen.add(top)
        return top
    heads = [pr["head"]["ref"] for pr in matches[0]["pull_requests"] if is_open(pr)]
    return heads[-1] if heads else matches[0]["base"]["ref"]


def stacked_base(runner: Runner, pr_number: str) -> str | None:
    """The head branch of the open PR below PR `pr_number` in its open GitHub stack (the stack's base for its first
    open PR), read from GitHub's stacks API, never from local gh-stack tracking; None when the PR is in no open stack."""
    for stack in open_stacks(runner):
        base = stack["base"]["ref"]
        for pr in stack["pull_requests"]:
            if str(pr["number"]) == str(pr_number):
                return base
            if is_open(pr):
                base = pr["head"]["ref"]
    return None


def dev_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    plan = runner([VALIDATE_PLAN, "--root", args.root], capture_output=True, text=True, cwd=str(PRIMARY))
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
    if (reason := refusal_for_difficulty(bead, members_for(runner), identity)):
        return reason
    if runner(["git", *git_dir(args), "merge-base", "--is-ancestor", f"origin/{args.pr_target}", "HEAD"], capture_output=True, text=True).returncode:
        return "WRONG_BASE"
    return "READY"


def sanity_gate(args: argparse.Namespace, runner: Runner, identity: str) -> str:
    if not args.pr_number or not args.commit:
        return "PR_REQUIRED"
    run_json(runner, "bd", "show", args.bead, "--json")
    pr = run_json(runner, "gh", "pr", "view", args.pr_number, "--json", "baseRefName,headRefName,headRefOid")
    base = str(pr.get("baseRefName") or "")
    stacked = stacked_base(runner, args.pr_number)
    # Layer 0 alone cannot form a stack: unlinked, it is based on the trunk (its pr_target, no open PR's head).
    awaiting_layer_one = stacked is None and base == args.pr_target and not open_pr_heads(runner, "--head", base)
    assigned = getattr(args, "base", "")
    if pr.get("headRefOid") != args.commit or (stacked != base and not awaiting_layer_one) or (assigned and base != assigned):
        return "NOT_STACKED"
    git = git_dir(args)
    run(runner, "git", *git, "fetch", "origin")
    if not descends(runner, args.pr_target, base, git):
        return "NOT_STACKED"
    if not run(runner, "git", *git, "log", "--format=%H", f"origin/{base}..{args.commit}"):
        return "ZERO_DELTA"
    rebased = runner(["git", *git, "merge-base", "--is-ancestor", f"origin/{base}", args.commit], capture_output=True, text=True)
    if rebased.returncode == 1:
        return "NOT_REBASED"
    if rebased.returncode:
        raise RuntimeError(rebased.stderr.strip() or "git merge-base failed")
    if not is_clean(run(runner, "git", *git, "status", "--porcelain", "--untracked-files=no")):
        return "DIRTY_TREE"
    if has_prior_pass(run(runner, "bd", "history", args.bead, "--json")):
        return "SANITY_FROZEN"
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
    parser.add_argument("--pr-target", required=True)
    parser.add_argument("--identity", default="")
    parser.add_argument("--pr-number", default="")
    parser.add_argument("--commit", default="")
    parser.add_argument("--checked-bead", default="")
    parser.add_argument("--worktree", default="")
    parser.add_argument("--base", default="", help="sanity: the assigned PR base; the PR's actual base must equal it")
    args = parser.parse_args(argv)
    if args.kind == "stack-top":
        try:
            top = stack_top(runner, args.pr_target)
        except (RuntimeError, OSError, TypeError, ValueError, KeyError):
            print("GATE_CANNOT_RUN")
            return 2
        print(top or "STACK_AMBIGUOUS")
        return 0 if top else 5
    if not (args.root and args.bead):
        parser.error(f"{args.kind} needs --root and --bead")
    code = evaluate(args, runner)
    print(code)
    return 0 if code == "READY" else (2 if code == "GATE_CANNOT_RUN" else 5)


if __name__ == "__main__":
    raise SystemExit(main())
