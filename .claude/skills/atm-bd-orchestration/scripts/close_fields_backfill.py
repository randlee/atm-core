#!/usr/bin/env python3
"""Stamp the close fields (close_fields.POURED) on a phase's open task beads (#557).

    close_fields_backfill.py --phase <x> [--apply] [--actor <identity>]

Beads poured before the formulas stamped close fields lack them, and bead_schema then rejects them. For every
open dev, fix, sanity and QA bead of the phase (and every in-progress finding, which the lead stamps at dispatch),
this derives the missing fields from the bead's own group: the checked bead, its sanity and QA beads, the sprint
container, the phase root and stack, and the repository configuration. It fills only missing or empty fields;
a field that is set and differs is reported, never overwritten. Each result is checked against bead_schema.

Without --apply it prints the plan. With --apply each bead's fill is one `update --set-metadata` journaled in
bd-cache (precondition: its status is unchanged), applied by the bd-cache drain or cron. The run refuses to write
anything when any bead cannot be fully stamped.
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
STAGE = {**close_fields.ROLE_BY_STAGE, "stage:finding": "fix"}


def stage_role(row: dict) -> str | None:
    roles = [STAGE[label] for label in row.get("labels") or [] if label in STAGE]
    return roles[0] if len(roles) == 1 else None


def needs_stamp(row: dict) -> bool:
    if row.get("status") == "closed":
        return False
    return "stage:finding" not in (row.get("labels") or []) or row.get("status") == "in_progress"


def member(rows: list[dict], label: str, key: str, checked: str | None) -> str | None:
    """The one open bead of `label` whose metadata[key] names `checked`; among several, the highest round."""
    found = [r for r in rows if label in (r.get("labels") or []) and (r.get("metadata") or {}).get(key) == checked]
    live = [r for r in found if r.get("status") != "closed"] or found
    live.sort(key=lambda r: ((r.get("metadata") or {}).get("round") or 0, r["id"]))
    return live[-1]["id"] if live else None


def plan(rows: list[dict], *, phase_feature: str, stack_bead: str, config: dict) -> tuple[list[dict], list[str]]:
    by_id = {row["id"]: row for row in rows}
    fills, problems = [], []
    for row in rows:
        role = stage_role(row)
        if role is None or not needs_stamp(row):
            continue
        meta = row.get("metadata") or {}
        checked = {"sanity": meta.get("dev_bead"), "qa": meta.get("checked_bead")}.get(role, row["id"])
        work = by_id.get(checked or "")
        if work is None:
            problems.append(f"{row['id']}: its checked bead {checked!r} is not in phase {meta.get('phase')!r}")
            continue
        wmeta = work.get("metadata") or {}
        sprint_bead = meta.get("sprint_bead") or wmeta.get("sprint_bead")
        container = by_id.get(sprint_bead or "")
        is_sprint_dev = "stage:dev" in (work.get("labels") or [])
        derived = {
            "role": role,
            **{k: meta.get(k) or wmeta.get(k) for k in CARRIED},
            "sprint_bead": sprint_bead,
            "stack_bead": stack_bead,
            "checked_bead": checked,
            "checked_title": (container or {}).get("title") if is_sprint_dev else work.get("title"),
            "sanity_bead": row["id"] if role == "sanity" else member(rows, "stage:dev-sanity", "dev_bead", checked),
            "qa_bead": row["id"] if role == "qa" else member(rows, "stage:qa", "checked_bead", checked),
            "phase_feature": phase_feature,
            "lint_command": config["lint_command"],
            "policy_path": config["policy_path"],
            "max_sanity_rounds": config["max_sanity_rounds"],
        }
        derived["base"] = derived["pr_target"]
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
            continue
        overlay = {**row, "metadata": {**meta, **fill}}
        issues = bead_schema.validate_phase([overlay])
        if issues:
            problems.extend(issues)
            continue
        if fill:
            fills.append({"bead": row["id"], "status": row.get("status") or "", "set_metadata": fill})
    return fills, problems


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
        report["fix"] = "Correct each named bead (or its group) with bd, then rerun; nothing was written."
    elif args.apply:
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
