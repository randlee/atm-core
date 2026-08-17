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
            ("python-tools", "Install the pinned Python validation/build/publish tools in a local venv."),
            ("ci", "Run the local CI-equivalent command set."),
            ("validate", "Run the full retained release validation suite."),
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
            ("lint unix-gating", "Run the Unix platform-gating lint subset."),
            ("lint runtime-waits", "Run the production Condvar wait lint subset."),
            ("lint sc-boundary", "Run the preliminary syn-based boundary analyzer."),
            ("lint sc-portability", "Run the preliminary syn-based portability analyzer."),
            ("lint manifests", "Run the Cargo manifest policy checks."),
            ("lint silent-emit", "Run the observability silent-discard regression gate."),
            ("lint function-length", "Run the RULE-002 function-length gate."),
            ("lint legacy-mailbox-paths", "Run the legacy mailbox/runtime deletion regression gate."),
            ("lint capability-degradation", "Run the replay capability no-degradation regression gate."),
            ("lint version", "Run only the version alignment checks."),
            ("lint identities", "Run the identity literal guard."),
            ("lint env-var-boundary", "Run the ATM_TEAM/ATM_IDENTITY client-boundary guard."),
            ("lint fixed-sleep", "Run the fixed thread::sleep test-hygiene gate."),
            ("lint ttl-triage", "Run the triage Turtle consistency gate."),
            ("lint lines", "Run only the RULE-003 line-count guard."),
            ("lint spell", "Run the spelling/content check."),
            ("lint daemon-singleton", "Run the daemon singleton/no-spawn test gate."),
            ("lint pytests", "Run the Python lint-tool unit tests."),
        ),
    ),
    (
        "Validate",
        (
            ("validate", "Run the full retained release preflight suite."),
            ("validate lint", "Run only the lint portion of release validation."),
            ("validate support-files", "Check required release support files."),
            ("validate manifest", "Run manifest / preflight-mode / publish-order checks."),
            ("validate publish-surface", "Run package / dry-run / unpublished-version checks."),
            ("validate release-binaries", "Check required release binaries in the manifest."),
            ("validate inventory", "Generate and validate a temporary release inventory."),
        ),
    ),
    (
        "Smoke",
        (
            ("smoke", "Run the normal fixture smoke harness."),
            ("smoke fast", "Run the fast fixture smoke harness."),
            ("smoke thorough", "Run the thorough fixture smoke harness."),
            ("smoke localhost", "Prove branch-daemon localhost send/read/ack."),
            ("smoke local-ip", "Add advertised-IP branch-daemon send/read/ack."),
            ("smoke crosshost <host...>", "Add inbound peer sends from SSH hostnames."),
            ("smoke peer-pair <args...>", "Run the host-supplied two-role release smoke."),
            ("smoke inbound-peer <args...>", "Run inbound peer evidence against an existing daemon."),
            ("smoke graft-hermes <args...>", "Run the full Hermes/PyO3 graft smoke."),
            ("benchmark <args...>", "Run the separate isolated performance gate."),
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
