#!/usr/bin/env python3
from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path


FORBIDDEN_LITERALS = ("team-lead", "arch-ctm", "atm-dev", "quality-mgr")

ALLOWED_PATTERNS = (
    re.compile(
        r"\b(?:pub\s+)?const\s+(?:ROLE_TEAM_LEAD|PRODUCTION_TEAM_LEAD|TEST_[A-Z0-9_]+)\b"
    ),
    re.compile(r"\b(?:ROLE_TEAM_LEAD|PRODUCTION_TEAM_LEAD|TEST_[A-Z0-9_]+)\b"),
    re.compile(r"\bATM_(?:TEAM|IDENTITY)\b"),
)


def is_test_scope(path: Path, text: str) -> bool:
    return "/tests/" in path.as_posix() or "#[cfg(test)]" in text or "mod tests" in text


def candidate_base_refs() -> list[str]:
    override = os.environ.get("RULE008_BASE_REF")
    refs = []
    if override:
        refs.append(override)
    refs.extend(["origin/integrate/phase-Q", "integrate/phase-Q", "origin/develop", "develop"])
    return refs


def resolve_diff_base(repo_root: Path) -> str | None:
    for ref in candidate_base_refs():
        show = subprocess.run(
            ["git", "rev-parse", "--verify", "--quiet", ref],
            cwd=repo_root,
        )
        if show.returncode != 0:
            continue
        merge_base = subprocess.run(
            ["git", "merge-base", "HEAD", ref],
            cwd=repo_root,
            check=False,
            capture_output=True,
            text=True,
        )
        if merge_base.returncode == 0:
            return merge_base.stdout.strip()
    return None


def collect_added_test_lines(repo_root: Path) -> list[tuple[Path, int, str]]:
    crate_root = repo_root / "crates"
    diff_base = resolve_diff_base(repo_root)
    if diff_base is None:
        raise SystemExit(
            "RULE-008 diff scope failed: could not resolve a merge base. "
            "Set RULE008_BASE_REF or ensure integrate/phase-Q or develop is available locally."
        )

    diff = subprocess.run(
        ["git", "diff", "--unified=0", "--no-color", diff_base, "--", "crates"],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.splitlines()

    results: list[tuple[Path, int, str]] = []
    current_path: Path | None = None
    current_line = 0
    current_is_test_scope = False
    hunk_line_pattern = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,\d+)? @@")

    for line in diff:
        if line.startswith("+++ b/"):
            current_path = Path(line.removeprefix("+++ b/"))
            if current_path.suffix != ".rs":
                current_path = None
                current_is_test_scope = False
                continue
            abs_path = repo_root / current_path
            text = abs_path.read_text(encoding="utf-8")
            current_is_test_scope = is_test_scope(current_path, text)
            continue
        if current_path is None or not current_is_test_scope:
            continue
        if match := hunk_line_pattern.match(line):
            current_line = int(match.group(1))
            continue
        if line.startswith("+") and not line.startswith("+++"):
            results.append((current_path, current_line, line[1:]))
            current_line += 1
            continue
        if line.startswith("-") and not line.startswith("---"):
            continue
        current_line += 1

    return results


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    failures: list[tuple[str, int, str]] = []
    scanned_lines = collect_added_test_lines(repo_root)
    for path, line_number, line in scanned_lines:
        if not any(literal in line for literal in FORBIDDEN_LITERALS):
            continue
        if any(pattern.search(line) for pattern in ALLOWED_PATTERNS):
            continue
        failures.append((path.as_posix(), line_number, line.strip()))

    if failures:
        print(
            "RULE-008 violation: raw production identity literals found in added test/cfg(test) lines."
        )
        print(
            "Use approved constants or explicit ATM_TEAM/ATM_IDENTITY env-var tests instead."
        )
        for path, line_number, line in failures:
            print(f"{path}:{line_number}: {line}")
        print(f"total violations: {len(failures)}")
        return 1

    print(
        "RULE-008 check passed: no disallowed raw production identity literals found in added test/cfg(test) lines."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
