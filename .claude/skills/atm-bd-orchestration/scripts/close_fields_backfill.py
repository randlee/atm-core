#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11,<3.12"
# dependencies = ["pydantic>=2", "pyyaml", "sc-compose>=1.8.0,<2"]
# ///
"""Stamp the close fields (close_fields.POURED) on a phase's open task beads (#557).

    close_fields_backfill.py --phase <x> [--apply] [--actor <identity>]

Beads poured before the formulas stamped close fields lack them, and bead_schema then rejects them. For every
open dev, fix, sanity and QA bead of the phase (and every in-progress finding, which the lead stamps at dispatch),
this derives the missing fields from the bead's own group: the checked bead, its sanity and QA beads, the sprint
container, the phase root and stack, and the repository configuration. It fills only missing or empty fields;
a field that is set and differs is reported, never overwritten. Each result is checked against bead_schema.

Without --apply it prints the plan. With --apply each bead's fill is one `update --set-metadata` journaled in
bd-cache (precondition: its status is unchanged), applied by the bd-cache drain or cron. A group with a problem
is not written; independent groups are still filled.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parents[1] / "atm-beads" / "scripts"))
import bd_cache  # noqa: E402
import bead_schema  # noqa: E402
import close_fields  # noqa: E402
import repo_config  # noqa: E402
import sprint_index_common  # noqa: E402
from stack_top import stack_bead_id  # noqa: E402

CARRIED = ("phase", "sprint", "sprint_bead", "stack", "branch", "worktree", "pr_target")
STAGE = close_fields.ROLE_BY_STAGE


def stage_role(row: dict) -> str | None:
    roles = [STAGE[label] for label in row.get("labels") or [] if label in STAGE]
    return roles[0] if len(roles) == 1 else None


def needs_stamp(row: dict) -> bool:
    if row.get("status") == "closed":
        return False
    return "stage:finding" not in (row.get("labels") or []) or row.get("status") == "in_progress"


class InvalidMember(ValueError):
    """A group's member cannot be selected safely from malformed metadata."""


def validate_round(row: dict) -> None:
    metadata = row.get("metadata") or {}
    if "round" in metadata and (type(metadata["round"]) is not int or metadata["round"] < 1):
        raise InvalidMember(f"{row['id']}: metadata.round must be a positive integer, got {metadata['round']!r}")


def member(rows: list[dict], label: str, key: str, checked: str | None, preferred: str | None = None) -> str | None:
    """Preserve an existing reference; otherwise select the highest-round live member."""
    found = [r for r in rows if label in (r.get("labels") or []) and (r.get("metadata") or {}).get(key) == checked]
    for row in found:
        validate_round(row)
    if preferred and any(r["id"] == preferred for r in found):
        return preferred
    live = [r for r in found if r.get("status") != "closed"]
    live.sort(key=lambda r: ((r.get("metadata") or {}).get("round") or 0, r["id"]))
    return live[-1]["id"] if live else None


def plan(rows: list[dict], *, phase_feature: str, stack_bead: str, config: dict) -> tuple[list[dict], list[str]]:
    by_id = {row["id"]: row for row in rows}
    fills, problems = [], []
    groups, invalid_groups = {}, set()
    for row in rows:
        role = stage_role(row)
        if role is None or not needs_stamp(row):
            continue
        meta = row.get("metadata") or {}
        checked = {"sanity": meta.get("dev_bead"), "qa": meta.get("checked_bead")}.get(role, row["id"])
        group = checked or row["id"]
        groups[row["id"]] = group
        try:
            if role in ("sanity", "qa"):
                validate_round(row)
        except InvalidMember as error:
            problems.append(str(error))
            invalid_groups.add(group)
            continue
        before = len(problems)
        work = by_id.get(checked or "")
        if work is None:
            problems.append(f"{row['id']}: its checked bead {checked!r} is not in phase {meta.get('phase')!r}")
            invalid_groups.add(group)
            continue
        wmeta = work.get("metadata") or {}
        sprint_bead = meta.get("sprint_bead") or wmeta.get("sprint_bead")
        container = by_id.get(sprint_bead or "")
        is_sprint_dev = "stage:dev" in (work.get("labels") or [])
        try:
            preferred_sanity = meta.get("sanity_bead")
            if role == "qa" and not preferred_sanity:
                work_sanity = wmeta.get("sanity_bead")
                if work_sanity and any(r.get("id") == work_sanity and r.get("status") != "closed" for r in rows):
                    preferred_sanity = work_sanity
            sanity_bead = row["id"] if role == "sanity" else member(rows, "stage:dev-sanity", "dev_bead", checked,
                                                                       preferred_sanity)
            qa_bead = row["id"] if role == "qa" else member(rows, "stage:qa", "checked_bead", checked)
        except InvalidMember as error:
            problems.append(str(error))
            invalid_groups.add(group)
            continue
        derived = {
            "role": role,
            **{k: meta.get(k) or wmeta.get(k) for k in CARRIED},
            "sprint_bead": sprint_bead,
            "stack_bead": stack_bead,
            "checked_bead": checked,
            "checked_title": (container or {}).get("title") if is_sprint_dev else work.get("title"),
            "sanity_bead": sanity_bead,
            "qa_bead": qa_bead,
            "phase_feature": phase_feature,
            "lint_command": config["lint_command"],
            "policy_path": config["policy_path"],
            "max_sanity_rounds": config["max_sanity_rounds"],
        }
        derived["base"] = None if "stage:finding" in (work.get("labels") or []) else derived["pr_target"]
        if "stage:finding" in (work.get("labels") or []):
            for related_id in (derived["sanity_bead"], derived["qa_bead"]):
                related = (by_id.get(related_id or "") or {}).get("metadata") or {}
                for name in ("branch", "worktree", "pr_target", "base"):
                    value = related.get(name)
                    if not value:
                        continue
                    current = derived.get(name)
                    if current and current != value:
                        problems.append(f"{row['id']}: metadata.{name} is {current!r} but {related_id} gives {value!r}; left unchanged")
                    elif not current:
                        derived[name] = value
        derived["base"] = derived["base"] or derived["pr_target"]
        if role == "qa" and meta.get("quick_fix") and not derived["sanity_bead"]:
            derived["sanity_bead"] = ""
        fill, lacking = {}, []
        for name in close_fields.POURED:
            have, want = meta.get(name), derived.get(name)
            if have not in (None, ""):
                if want not in (None, "") and have != want:
                    problems.append(f"{row['id']}: metadata.{name} is {have!r} but its group gives {want!r}; left unchanged")
                continue
            if want in (None, ""):
                if not (name == "sanity_bead" and role == "qa" and meta.get("quick_fix")):
                    lacking.append(name)
                continue
            fill[name] = want
        if lacking:
            problems.append(f"{row['id']}: cannot derive {', '.join(lacking)} from its group")
            invalid_groups.add(group)
            continue
        overlay = {**row, "metadata": {**meta, **fill}}
        issues = bead_schema.validate_phase([overlay])
        if issues:
            problems.extend(issues)
            invalid_groups.add(group)
            continue
        if len(problems) != before:
            invalid_groups.add(group)
            continue
        if fill:
            fills.append({"bead": row["id"], "status": row.get("status") or "", "set_metadata": fill})
    fix = ("Fix: for a finding run `bd update <finding> --status in_progress --assignee <dev>`, then "
           "`bd update <finding> --metadata <full JSON plus branch, worktree, pr_target and base = pr_target>`; "
           "for another bead correct its named fields with `bd update <bead> --metadata <full JSON>`. "
           "Run `close_fields_backfill.py --phase <x> --apply` and "
           "`uv run --script .claude/skills/atm-bd-orchestration/scripts/bd_cache.py --root . drain`.")
    return [fill for fill in fills if groups[fill["bead"]] not in invalid_groups], [f"{problem}; {fix}" for problem in problems]


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="close_fields_backfill.py", description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--phase", required=True)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--actor", default=os.environ.get("ATM_IDENTITY", ""))
    args = parser.parse_args(argv)
    repo = repo_config.repo_root()
    try:
        config = repo_config.load(repo)
        root = sprint_index_common.load_phase_config(repo, args.phase)["root"]
        cache = bd_cache.BdCache(repo)
        cache.sync(args.phase)
        root_row = cache.get(root)
        if not root_row.get("title"):
            raise SystemExit(f"close_fields_backfill: phase root {root} has no title for phase_feature")
        fills, problems = plan(cache.list(args.phase), phase_feature=root_row["title"], stack_bead=stack_bead_id(root),
                               config=config)
    except (bd_cache.CacheError, repo_config.ConfigError, RuntimeError) as error:
        print(json.dumps({"ok": False, "error": str(error)}, indent=2))
        return 2
    report = {"ok": not problems, "phase": args.phase, "apply": args.apply, "beads": len(fills), "fills": fills,
              "problems": problems, "journaled": []}
    if problems:
        report["fix"] = "Correct each named group and rerun; valid independent groups are still journaled."
    if args.apply:
        if not args.actor:
            report.update(ok=False, problems=["--actor (or ATM_IDENTITY) is required to journal writes"])
        else:
            for fill in fills:
                digest = hashlib.sha256(json.dumps(fill["set_metadata"], sort_keys=True).encode()).hexdigest()[:16]
                result = cache.write(fill["bead"], "update", {"set_metadata": fill["set_metadata"]},
                                     idem_key=f"close-fields-backfill:{fill['bead']}:{digest}", actor=args.actor,
                                     precondition={"status": fill["status"]})
                report["journaled"].append({"bead": fill["bead"], **result})
    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
