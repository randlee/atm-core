#!/usr/bin/env python3
"""Reject tracked sc-compose templates whose front matter is missing or does not declare every variable they use."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

from lint_common import discover_repo_root


# The installed atm-bd-orchestration copy is fixed upstream, never edited here.
EXCLUDED_PREFIXES = (".claude/skills/atm-bd-orchestration/",)
# sc-compose 1.6.1 reports these Jinja keywords as undeclared tokens
# (randlee/sc-compose#784). Drop this set when that issue is fixed.
SC_COMPOSE_784_TOKENS = frozenset({"autoescape", "endautoescape", "is", "defined"})
UNDECLARED = re.compile(r"ERR_VAL_UNDECLARED_TOKEN: undeclared referenced token: (\S+)")
MISSING_FRONTMATTER = "ERR_VAL_MISSING_FRONTMATTER"


def tracked_templates(repo_root: Path) -> list[str]:
    listed = subprocess.run(
        ["git", "-C", str(repo_root), "ls-files", "*.j2"], capture_output=True, text=True, check=True
    ).stdout.splitlines()
    return [name for name in listed if not name.startswith(EXCLUDED_PREFIXES)]


def template_violations(repo_root: Path, template: str) -> list[str]:
    """Run sc-compose's strict validation and keep only the front-matter findings."""
    completed = subprocess.run(
        ["sc-compose", "validate", "--strict", "--root", str(repo_root), "--file", template],
        cwd=repo_root, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    violations: list[str] = []
    for line in (completed.stdout + completed.stderr).splitlines():
        if MISSING_FRONTMATTER in line:
            violations.append(f"{template}: no front matter declaring its variables")
            continue
        token = UNDECLARED.search(line)
        if token and token.group(1) not in SC_COMPOSE_784_TOKENS:
            violations.append(f"{template}: variable `{token.group(1)}` is used but not declared in front matter")
    return violations


def find_violations(repo_root: Path, templates: list[str] | None = None) -> list[str]:
    violations: list[str] = []
    for template in tracked_templates(repo_root) if templates is None else templates:
        violations.extend(template_violations(repo_root, template))
    return violations


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", help="Repository root to inspect.")
    args = parser.parse_args(argv[1:])
    violations = find_violations(discover_repo_root(args.root))
    if violations:
        print("template front matter violations:")
        print("\n".join(f"- {violation}" for violation in violations))
        return 1
    print("template front matter: every tracked *.j2 declares the variables it uses")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
