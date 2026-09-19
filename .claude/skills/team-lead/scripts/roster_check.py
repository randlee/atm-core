#!/usr/bin/env python3
"""Read-only three-way check: `.atm.toml` aliases, the ATM roster, live Herdr agents.

Prints one row per roster member and one line per problem. Exits 1 when any
problem is found, 2 when a source cannot be read. It never changes anything;
the team-lead skill lists the repair command for each problem.

Usage: roster_check.py [--team TEAM] [--atm-toml PATH]
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

# Every project names these members the same way, so their bare names collide
# across teams on one Herdr session. They must carry a unique alias.
ALIAS_REQUIRED = ("team-lead", "quality-mgr", "publisher")


def config_aliases(atm_toml: dict, team: str) -> dict[str, str | None]:
    """Member name -> alias declared in `.atm.toml` for panes of this team."""
    found: dict[str, str | None] = {}
    for window in atm_toml.get("rmux", {}).get("windows", []):
        for pane in window.get("panes", []):
            env = pane.get("env", {})
            if env.get("ATM_TEAM") == team and "ATM_IDENTITY" in env:
                found[env["ATM_IDENTITY"]] = pane.get("alias")
    return found


def find_problems(
    config: dict[str, str | None], roster: list[dict], herdr_names: list[str]
) -> list[str]:
    problems: list[str] = []
    live = set(herdr_names)
    for member in roster:
        name, alias = member["name"], member.get("alias")
        declared = config.get(name)
        if name in ALIAS_REQUIRED and not alias:
            problems.append(f"{name}: roster alias missing (required for a unique Herdr name)")
        if name in ALIAS_REQUIRED and not declared:
            problems.append(f"{name}: .atm.toml declares no alias (required)")
        if declared != alias and (declared or alias):
            problems.append(f"{name}: .atm.toml alias {declared!r} != roster alias {alias!r}")
        if member.get("backend") == "herdr" and (alias or name) not in live:
            problems.append(f"{name}: no live Herdr agent named {(alias or name)!r}")
    roster_names = {member["name"] for member in roster}
    for name in sorted(set(config) - roster_names):
        problems.append(f"{name}: declared in .atm.toml but not in the roster")
    for name in sorted(roster_names - set(config)):
        problems.append(f"{name}: in the roster but has no pane in .atm.toml")
    for name in sorted({n for n in herdr_names if herdr_names.count(n) > 1}):
        problems.append(f"Herdr agent name {name!r} is not unique on this session")
    return problems


def run_json(command: list[str]) -> dict:
    completed = subprocess.run(command, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise RuntimeError(f"{' '.join(command)} failed: {completed.stderr.strip()}")
    return json.loads(completed.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--team", default=os.environ.get("ATM_TEAM", ""))
    parser.add_argument("--atm-toml", default=".atm.toml")
    args = parser.parse_args()
    if not args.team:
        print("roster_check: set ATM_TEAM or pass --team", file=sys.stderr)
        return 2
    try:
        config = config_aliases(tomllib.loads(Path(args.atm_toml).read_text("utf-8")), args.team)
        roster = run_json(["atm", "members", "--team", args.team, "--json"])["members"]
        agents = run_json(["herdr", "agent", "list"])["result"]["agents"]
    except (OSError, RuntimeError, KeyError, ValueError) as error:
        print(f"roster_check: {error}", file=sys.stderr)
        return 2
    herdr_names = [agent["name"] for agent in agents if agent.get("name")]

    print(f"{'member':<14}{'.atm.toml alias':<18}{'roster alias':<16}{'herdr agent':<16}state")
    for member in roster:
        target = member.get("alias") or member["name"]
        print(
            f"{member['name']:<14}{config.get(member['name']) or '-':<18}"
            f"{member.get('alias') or '-':<16}"
            f"{target if target in herdr_names else 'MISSING':<16}{member.get('state', '-')}"
        )
    problems = find_problems(config, roster, herdr_names)
    print()
    for problem in problems:
        print(f"PROBLEM {problem}")
    print("OK: .atm.toml, roster and Herdr agree" if not problems else f"{len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
