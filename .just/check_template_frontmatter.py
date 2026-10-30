#!/usr/bin/env python3
"""Reject tracked sc-compose templates whose front matter is missing or does not declare every variable they use."""

from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

from lint_common import discover_repo_root


# sc-compose 1.7.0 reports these Jinja keywords as undeclared tokens
# (randlee/sc-compose#784). Drop this set when that issue is fixed.
SC_COMPOSE_784_TOKENS = frozenset({"autoescape", "endautoescape", "is", "defined"})
UNDECLARED = re.compile(r"ERR_VAL_UNDECLARED_TOKEN: undeclared referenced token: (\S+)")
MISSING_FRONTMATTER = "ERR_VAL_MISSING_FRONTMATTER"

def tracked_templates(repo_root: Path) -> list[str]:
    listed = subprocess.run(
        ["git", "-C", str(repo_root), "ls-files", "*.j2"], capture_output=True, text=True, check=True
    ).stdout.splitlines()
    return listed


def template_violations(repo_root: Path, template: str) -> list[str]:
    """Run strict validation, retaining front-matter findings and unknown failures."""
    completed = subprocess.run(
        ["sc-compose", "validate", "--strict", "--root", str(repo_root), "--file", template],
        cwd=repo_root, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
    )
    output = completed.stdout + completed.stderr
    violations: list[str] = []
    for line in output.splitlines():
        if MISSING_FRONTMATTER in line:
            violations.append(f"{template}: no front matter declaring its variables")
            continue
        token = UNDECLARED.search(line)
        if token:
            if token.group(1) in SC_COMPOSE_784_TOKENS:
                continue
            else:
                violations.append(
                    f"{template}: variable `{token.group(1)}` is used but not declared in front matter"
                )
    if completed.returncode and not violations:
        diagnostic_codes = re.findall(r"ERR_[A-Z0-9_]+", output)
        recognized_codes = {
            "ERR_VAL_MISSING_FRONTMATTER",
            "ERR_VAL_MISSING_REQUIRED",
            "ERR_VAL_UNDECLARED_TOKEN",
        }
        if not diagnostic_codes or any(code not in recognized_codes for code in diagnostic_codes):
            violations.append(
                f"{template}: sc-compose strict validation failed (exit {completed.returncode}):\n"
                f"{output.rstrip()}"
            )
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
    print("template front matter: every repo-owned tracked *.j2 declares the variables it uses")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
