#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path


SECTIONS = (
    (
        "General",
        (
            ("help", "Show this help."),
            ("build", "Build the full workspace."),
            ("test", "Run the full workspace test suite."),
            ("clean", "Remove workspace build artifacts."),
            ("ci", "Run the local CI-equivalent command set."),
        ),
    ),
    (
        "Formatting",
        (
            ("fmt", "Check Rust formatting."),
            ("fmt check", "Check Rust formatting."),
            ("fmt write", "Format the Rust workspace in place."),
            ("fmt apply", "Format the Rust workspace in place."),
        ),
    ),
    (
        "Lint",
        (
            ("lint", "Run the full repo lint suite."),
            ("lint fmt", "Run only the format check."),
            ("lint clippy", "Run only Clippy with warnings denied."),
            ("lint version", "Run only the version alignment checks."),
            ("lint identities", "Run only the RULE-008 identity literal guard."),
            ("lint lines", "Run only the RULE-003 line-count guard."),
        ),
    ),
)


def main() -> int:
    repo_name = Path(__file__).resolve().parent.parent.name
    print(f"{repo_name} task runner")
    print()
    print("Usage:")
    print("  just <recipe>")
    print()
    width = max(len(name) for _, recipes in SECTIONS for name, _ in recipes)
    for section_name, recipes in SECTIONS:
        print(f"{section_name}:")
        for name, description in recipes:
            print(f"  {name.ljust(width)}  {description}")
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
