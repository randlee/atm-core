#!/usr/bin/env python3
from __future__ import annotations

from pathlib import Path


RECIPES = (
    ("help", "Show this help."),
    ("fmt", "Format the Rust workspace in place."),
    ("fmt-check", "Check Rust formatting."),
    ("clippy", "Run Clippy with warnings denied."),
    ("version-check", "Verify crate, lockfile, winget, and Homebrew release wiring stay aligned."),
    ("lint-identities", "Enforce RULE-008 for raw production identity literals in test scope."),
    ("lint-lines", "Enforce RULE-003 source file size limits."),
    ("lint", "Run the full repo lint suite."),
    ("build", "Build the full workspace."),
    ("test", "Run the full workspace test suite."),
    ("clean", "Remove workspace build artifacts."),
    ("ci", "Run the local CI-equivalent command set."),
)


def main() -> int:
    repo_name = Path(__file__).resolve().parent.parent.name
    print(f"{repo_name} task runner")
    print()
    print("Usage:")
    print("  just <recipe>")
    print()
    print("Recipes:")
    width = max(len(name) for name, _ in RECIPES)
    for name, description in RECIPES:
        print(f"  {name.ljust(width)}  {description}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
