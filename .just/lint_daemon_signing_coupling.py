#!/usr/bin/env python3
"""Ensure every Justfile daemon build invokes the development signer."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import re
import shlex
import sys

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report


LINT_NAME = "daemon-signing-coupling"
RECIPE_HEADER_RE = re.compile(
    r"^(?P<name>[A-Za-z_][A-Za-z0-9_-]*)(?:\s+[^:]+)?\s*:(?![=]).*$"
)
DAEMON_BUILD_RE = re.compile(r"\bcargo\s+build\b")
SIGNING_HOOK = ".just/sign_daemon_dev.py"


@dataclass(frozen=True)
class SigningViolation:
    recipe: str
    line_number: int

    def render(self) -> str:
        return (
            f"Justfile:{self.line_number}: recipe `{self.recipe}` builds atm-daemon "
            f"without invoking {SIGNING_HOOK}"
        )


def recipe_blocks(text: str) -> list[tuple[str, int, list[tuple[int, str]]]]:
    """Return recipe name, header line, and indented body lines."""
    blocks: list[tuple[str, int, list[tuple[int, str]]]] = []
    current_name: str | None = None
    current_header = 0
    current_body: list[tuple[int, str]] = []

    def finish() -> None:
        nonlocal current_name, current_header, current_body
        if current_name is not None:
            blocks.append((current_name, current_header, current_body))
        current_name = None
        current_header = 0
        current_body = []

    for line_number, line in enumerate(text.splitlines(), start=1):
        header = RECIPE_HEADER_RE.match(line)
        if header:
            finish()
            current_name = header.group("name")
            current_header = line_number
            continue
        if current_name is None:
            continue
        if line[:1].isspace() or not line.strip() or line.lstrip().startswith("#"):
            current_body.append((line_number, line))
            continue
        finish()
    finish()
    return blocks


def collect_violations(repo_root: Path) -> list[SigningViolation]:
    justfile = repo_root / "Justfile"
    violations: list[SigningViolation] = []
    for recipe, _header_line, body in recipe_blocks(justfile.read_text(encoding="utf-8")):
        daemon_build_lines = [
            (line_number, line)
            for line_number, line in body
            if not line.lstrip().startswith("#") and is_daemon_build(line)
        ]
        if daemon_build_lines and not any(SIGNING_HOOK in line for _line_number, line in body):
            for line_number, _line in daemon_build_lines:
                violations.append(SigningViolation(recipe, line_number))
    return violations


def is_daemon_build(line: str) -> bool:
    """Classify workspace/unfiltered builds and explicit atm-daemon builds."""
    if not DAEMON_BUILD_RE.search(line):
        return False
    try:
        tokens = shlex.split(line)
    except ValueError:
        return False

    packages: list[str] = []
    for index, token in enumerate(tokens):
        if token in {"-p", "--package"} and index + 1 < len(tokens):
            packages.append(tokens[index + 1])
        elif token.startswith("-p="):
            packages.append(token[3:])
        elif token.startswith("--package="):
            packages.append(token.split("=", 1)[1])
    return not packages or "atm-daemon" in packages


def run(repo_root: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    violations = collect_violations(repo_root)
    duration_seconds = monotonic_now() - started_monotonic
    findings = [violation.render() for violation in violations]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "all Justfile daemon builds invoke the signing hook"
            if not violations
            else f"{len(violations)} daemon build recipe(s) omit the signing hook"
        ),
        findings=findings,
        transcript_lines=findings or ["no daemon signing coupling violations found"],
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
    return 0 if report.passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", help="Repo root to inspect.")
    args = parser.parse_args(argv[1:])
    return run(discover_repo_root(args.root))


if __name__ == "__main__":
    sys.exit(main(sys.argv))
