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
            ("version", "Show current workspace version state."),
            ("version latest", "Show recommended direct dependency upgrades."),
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
            ("lint fast", "Run the low-latency lint subset."),
            ("lint fmt", "Run only the format check."),
            ("lint clippy", "Run only Clippy with warnings denied."),
            ("lint modules", "Run cargo-modules internal acyclic checks (advisory/manual)."),
            ("lint deny", "Run cargo-deny advisories/bans/source checks."),
            ("lint shear", "Run cargo-shear unused-dependency checks."),
            ("lint boundaries", "Run the crate/source boundary checks."),
            ("lint manifests", "Run the Cargo manifest policy checks."),
            ("lint version", "Run only the version alignment checks."),
            ("lint identities", "Run only the RULE-008 identity literal guard."),
            ("lint lines", "Run only the RULE-003 line-count guard."),
            ("lint spell", "Run the spelling/content check."),
            ("lint daemon-singleton", "Run the daemon singleton/no-spawn test gate."),
            ("lint pytests", "Run the Python lint-tool unit tests."),
        ),
    ),
    (
        "View",
        (
            ("view", "Generate all implemented architecture-view artifacts."),
            ("view boundaries", "Generate boundary inventory artifacts."),
            ("view lines", "Generate source-size inventory artifacts."),
            ("view modules", "Generate module-structure artifacts."),
            ("view deps", "Generate crate-dependency HTML artifacts."),
            ("view unsafe", "Generate unsafe-surface artifacts."),
        ),
    ),
)


def render_help(repo_name: str) -> str:
    lines = [
        f"{repo_name} task runner",
        "",
        "Usage:",
        "  just <recipe>",
        "",
    ]
    width = max(len(name) for _, recipes in SECTIONS for name, _ in recipes)
    for section_name, recipes in SECTIONS:
        lines.append(f"{section_name}:")
        for name, description in recipes:
            lines.append(f"  {name.ljust(width)}  {description}")
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    repo_name = Path(__file__).resolve().parent.parent.name
    print(render_help(repo_name), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
