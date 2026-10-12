#!/usr/bin/env python3
"""Delegate identity checks to the published CLI (delete when randlee/sc-lint#198 ships)."""
from __future__ import annotations

import os
from pathlib import Path
import re
import sys
import tomllib


# delete when randlee/sc-lint#347 ships: the published identity rule does not
# yet reconstruct literal fragments in Rust concat! calls.
CONCAT_RE = re.compile(
    r'\bconcat!\s*\(\s*((?:"(?:\\.|[^"\\])*"\s*,\s*)+"(?:\\.|[^"\\])*")\s*,?\s*\)',
    re.DOTALL,
)
STRING_RE = re.compile(r'"((?:\\.|[^"\\])*)"')


def concat_identity_literals_are_clean(repo_root: Path) -> bool:
    config = tomllib.loads((repo_root / ".just/lint-config.toml").read_text(encoding="utf-8"))
    forbidden = config.get("identities", {}).get("forbidden_literals", [])
    for path in repo_root.glob("crates/**/*.rs"):
        for match in CONCAT_RE.finditer(path.read_text(encoding="utf-8")):
            fragments = STRING_RE.findall(match.group(1))
            value = "".join(fragments)
            if any(
                literal in value and not any(literal in fragment for fragment in fragments)
                for literal in forbidden
            ):
                print(f"identity concat gap: {path.relative_to(repo_root)} contains a forbidden literal", file=sys.stderr)
                return False
    return True


def main(argv: list[str]) -> int:
    repo_root = Path(__file__).resolve().parent.parent
    if not concat_identity_literals_are_clean(repo_root):
        return 1
    os.chdir(repo_root)
    os.execvp("sc-lint", ["sc-lint", "lint", "identity-literals", "--root", str(repo_root), *argv[1:]])
    return 127


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
