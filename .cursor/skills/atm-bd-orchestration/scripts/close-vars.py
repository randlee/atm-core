#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.12"
# dependencies = ["pydantic>=2", "pyyaml", "sc-compose>=1.8.0,<2"]
# ///
"""Print the close vars of one poured dev, fix, sanity or QA bead (#557).

    close-vars.py <bead-id> --out <vars.json> [--repo <beads repository root>]
    close-vars.py <qa-bead-id> --qa-assignment --out <vars.json> [--repo <beads repository root>]

Every validated open task bead carries its close fields (close_fields.POURED), so the vars come from that one
bead: the poured fields, the task ids (equal to the bead ids) and the host paths. A sanity close adds its round
(the task's assignment events) and the reviewers of its QA bead; a QA close adds its round. Beads are read from the
bd-cache (pending journal writes replayed on top); the close hooks write through it, so it holds their recorded
fields. When a field the vars need is missing there, the beads are re-read from bd once (`bd list --id`) and the
vars rebuilt; `bd show` is used when the cache cannot be opened. Each fallback is named in `read_fallback`.

It writes `--out` and prints JSON: role, the fields the closing agent still supplies (`remaining`) and the
manual close commands. A bead missing a poured field is refused with the fix; never assemble its fields from
other beads.

`--qa-assignment` instead writes the `templates.qa` vars of a QA bead: its poured fields plus the current PR's
head SHA, base ref, branch and number (close_fields.qa_assignment_vars), and prints the `atm task assign` command.
A checked bead with no recorded PR or no readable current PR is refused, never sent with stale first-close context.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1] / "atm-beads" / "scripts"))
import bd_cache  # noqa: E402
import close_fields  # noqa: E402
import task_close_validation  # noqa: E402

ATM_TIMEOUT_S = 10
BD_SHOW_TIMEOUT_S = 30
GH_PR_TIMEOUT_S = 30


# Refusals that a cached row lagging a direct `bd update` could cause; a fresh read from bd settles them.
MISSING = ("CLOSE_VARS.NOT_POURED", "ASSIGN_VARS.NOT_RECORDED", "ASSIGN_VARS.NOT_PLANNED")
NOT_POURED_FIX = ("For a finding, run `bd update <finding> --status in_progress --assignee <dev>`, then "
                  "`bd update <finding> --metadata <full JSON plus branch, worktree, pr_target and base = pr_target>`; "
                  "run `close_fields_backfill.py --phase <x> --apply` and "
                  "`uv run --script .claude/skills/atm-bd-orchestration/scripts/bd_cache.py --root . drain`, then retry.")


def read(repo: Path, bead: str, fallback: list[str], cache_factory=bd_cache.BdCache, *, fresh: bool = False) -> dict:
    """The bead from the bd-cache with pending journal writes replayed on top; `fresh` reads bd first.

    A failed fresh read falls back to the cached row, and a cache that cannot be opened to `bd show`.
    """
    try:
        cache = cache_factory(repo)
    except (bd_cache.CacheError, OSError) as error:
        fallback.append(f"bd-cache: {error}")
        return show(repo, bead)
    if fresh:
        try:
            row = cache.bd_rows([bead]).get(bead)
        except (bd_cache.CacheError, OSError) as error:
            fallback.append(f"bd list --id {bead} failed, using the cached row: {error}")
        else:
            if row is None:
                raise close_fields.CloseFieldsError("CLOSE_VARS.READ_FAILED", f"{bead}: not found in bd",
                                                    "Check the bead id and that bd works here.")
            cache.store(row, force=True)
            return cache.view(row)
    try:
        return cache.get(bead)
    except (bd_cache.CacheError, OSError) as error:
        fallback.append(f"bd-cache: {error}")
        return show(repo, bead)


def show(repo: Path, bead: str) -> dict:
    try:
        shown = subprocess.run(["bd", "show", bead, "--json"], cwd=repo, capture_output=True, text=True,
                               timeout=BD_SHOW_TIMEOUT_S, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.READ_FAILED", f"{bead}: bd show failed: {error}",
                                            "Check that bd and its dolt server answer here (`bd version`), then rerun.") from error
    if shown.returncode != 0:
        raise close_fields.CloseFieldsError("CLOSE_VARS.READ_FAILED", f"{bead}: bd show exited {shown.returncode}: "
                                            f"{(shown.stderr or shown.stdout).strip()[-300:]}", "Check the bead id and that bd works here.")
    try:
        rows = json.loads(shown.stdout)
        return rows[0] if isinstance(rows, list) else rows
    except (ValueError, IndexError) as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.READ_FAILED", f"{bead}: bd show printed no bead: {error}",
                                            "Check the bead id and the bd version.") from error


def task_events(task: str) -> list[dict]:
    try:
        events = subprocess.run(["atm", "task", "events", task, "--all", "--json"], capture_output=True, text=True,
                                timeout=ATM_TIMEOUT_S, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.EVENTS_FAILED", f"atm task events {task} failed: {error}",
                                            "Check that the ATM daemon answers (`atm task list`), then rerun.") from error
    if events.returncode != 0:
        raise close_fields.CloseFieldsError("CLOSE_VARS.EVENTS_FAILED", f"atm task events {task} exited {events.returncode}: "
                                            f"{(events.stderr or events.stdout).strip()[-300:]}", "Check that atm can read this task.")
    try:
        history = json.loads(events.stdout)["events"]
    except (ValueError, KeyError, TypeError) as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.EVENTS_FAILED", f"atm task events {task} printed no events: {error}",
                                            "Check the atm version.") from error
    if not any(event.get("event") == "started" for event in history):
        raise close_fields.CloseFieldsError("CLOSE_VARS.NOT_STARTED", f"{task} has no started event",
                                            f"Run `atm task start {task} \"<plan>\"` first.")
    return history


def sanity_round_from_events(task: str, events: list[dict]) -> int:
    """Use the close hook's shared assignment-event producer for the sanity round."""
    try:
        _, _, round_number = task_close_validation.assignment_details(events, task)
    except task_close_validation.CloseError as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.EVENTS_FAILED", str(error), error.fix) from error
    return round_number


def build(repo: Path, bead: str, *, events_for_task=task_events) -> dict:
    return cached_then_fresh(lambda fresh, fallback: _build(repo, bead, events_for_task, fresh, fallback))


def _build(repo, bead, events_for_task, fresh, fallback):
    row = read(repo, bead, fallback, fresh=fresh)
    role, values, remaining = close_fields.close_vars(row, repo)
    metadata = row.get("metadata") or {}
    derived = {}
    if role == "qa":
        derived["round"] = metadata.get("round")
    if role == "sanity":
        derived["round"] = sanity_round_from_events(bead, events_for_task(bead))
        derived["reviewers_round1"] = (read(repo, metadata["qa_bead"], fallback, fresh=fresh).get("metadata") or {}).get("reviewers_round1")
    for name, value in derived.items():
        if name in remaining and value not in (None, "", []):
            values[name] = value
            remaining.remove(name)
    return {"bead": bead, "role": role, "vars": values, "remaining": remaining, "read_fallback": fallback}


def read_current_pr(repo: Path, number: str) -> dict:
    argv = ["gh", "pr", "view", number, "--json", "number,baseRefName,headRefName,headRefOid"]
    try:
        result = subprocess.run(argv, cwd=repo, capture_output=True, text=True, timeout=GH_PR_TIMEOUT_S, check=False)
    except (OSError, subprocess.TimeoutExpired) as error:
        raise close_fields.CloseFieldsError(
            "ASSIGN_VARS.PR_READ_FAILED", f"gh pr view {number} failed: {error}",
            f"Check GitHub access and retry `gh pr view {number} --json number,baseRefName,headRefName,headRefOid`; "
            "do not use first-close base or commit values.",
        ) from error
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()[-300:]
        raise close_fields.CloseFieldsError(
            "ASSIGN_VARS.PR_READ_FAILED", f"gh pr view {number} exited {result.returncode}: {detail}",
            f"Check that PR #{number} is readable here and retry `gh pr view {number} --json "
            "number,baseRefName,headRefName,headRefOid`; do not use first-close base or commit values.",
        )
    try:
        row = json.loads(result.stdout)
    except ValueError as error:
        raise close_fields.CloseFieldsError(
            "ASSIGN_VARS.PR_READ_FAILED", f"gh pr view {number} printed invalid JSON: {error}",
            f"Retry `gh pr view {number} --json number,baseRefName,headRefName,headRefOid` and inspect its output.",
        ) from error
    if not isinstance(row, dict):
        raise close_fields.CloseFieldsError(
            "ASSIGN_VARS.PR_READ_FAILED", f"gh pr view {number} did not print a PR object",
            f"Retry `gh pr view {number} --json number,baseRefName,headRefName,headRefOid` and inspect its output.",
        )
    return {"number": row.get("number"), "base": row.get("baseRefName"),
            "head": row.get("headRefName"), "head_sha": row.get("headRefOid")}


def build_qa_assignment(repo: Path, bead: str, *, pr_reader=None) -> dict:
    reader = pr_reader or read_current_pr
    return cached_then_fresh(lambda fresh, fallback: _build_qa_assignment(repo, bead, fresh, fallback, reader))


def _build_qa_assignment(repo, bead, fresh, fallback, pr_reader):
    row = read(repo, bead, fallback, fresh=fresh)
    checked = (row.get("metadata") or {}).get("checked_bead")
    if not checked:
        raise close_fields.CloseFieldsError("CLOSE_VARS.NOT_POURED", f"{bead}: metadata lacks checked_bead",
                                            "Ask the phase lead to run close_fields_backfill.py for its phase.")
    checked_row = read(repo, checked, fallback, fresh=fresh)
    checked_meta = checked_row.get("metadata") or {}
    recorded_number = checked_meta.get("pr_number")
    current_pr = pr_reader(repo, str(recorded_number)) if recorded_number not in (None, "") else None
    values = close_fields.qa_assignment_vars(row, checked_row, repo, current_pr)
    return {"bead": bead, "role": "qa-assignment", "vars": values, "remaining": [], "read_fallback": fallback}


def cached_then_fresh(attempt) -> dict:
    """Build from cached rows; a refusal for a missing field is retried once on rows read fresh from bd."""
    fallback: list[str] = []
    try:
        return attempt(False, fallback)
    except close_fields.CloseFieldsError as error:
        if error.code not in MISSING:
            raise
        fallback.append(f"{error.code} on cached rows ({error}); informational: the cache lagged bd; the row was re-read; no action needed")
        return attempt(True, fallback)


def assign_command(repo: Path, values: dict, out: str) -> list[str]:
    import repo_config  # noqa: E402
    import sprint_index_common  # noqa: E402
    try:
        template = sprint_index_common.load_phase_config(repo, values["phase"])["templates"]["qa"]
        member = repo_config.load(repo)["qa_member"]
    except (RuntimeError, repo_config.ConfigError, KeyError) as error:
        raise close_fields.CloseFieldsError("CLOSE_VARS.CONFIG", f"cannot name the QA template or member: {error}",
                                            f"Fix .atm-bd/phase-{values['phase']}.toml templates.qa or qa_member in "
                                            ".claude/project/atm-bd-orchestration.yaml.") from error
    return [f"atm task assign {member} --task-id {values['task_id']} --template {template} --vars {out}"]


def commands(role: str, bead: str, out: str) -> list[str]:
    close = ".claude/skills/atm-bd-orchestration/scripts/atm-task-close.py"
    if role in ("dev", "fix"):
        return [f"write the remaining fields to <result.json>",
                f"{close} --role {role} --vars {out} --result <result.json>"]
    return [f"add the remaining fields to {out}", f"{close} --role {role} --vars {out}"]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="close-vars.py", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("bead")
    parser.add_argument("--out", required=True)
    parser.add_argument("--repo", default=".")
    parser.add_argument("--qa-assignment", action="store_true", help="write the templates.qa vars of a QA bead")
    args = parser.parse_args(argv)
    repo = Path(args.repo).resolve()
    try:
        result = build_qa_assignment(repo, args.bead) if args.qa_assignment else build(repo, args.bead)
        steps = (assign_command(repo, result["vars"], args.out) if args.qa_assignment
                 else commands(result["role"], args.bead, args.out))
    except close_fields.CloseFieldsError as error:
        report = error.as_json()
        if error.code == "CLOSE_VARS.NOT_POURED":
            report["fix"] = NOT_POURED_FIX
        print(json.dumps({"ok": False, "error": report}, indent=2))
        return 2
    for message in result["read_fallback"]:
        print(f"close-vars: {message}", file=sys.stderr)
    Path(args.out).write_text(json.dumps(result["vars"], indent=2) + "\n")
    print(json.dumps({"ok": True, "bead": result["bead"], "role": result["role"], "vars_path": args.out,
                      "remaining": result["remaining"], "read_fallback": result["read_fallback"],
                      "next": steps}, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
