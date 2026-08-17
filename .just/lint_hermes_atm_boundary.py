#!/usr/bin/env python3
"""Enforce the pure-Python hermes-atm boundary declared in its TOML record."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import sys
import tomllib

from lint_common import build_report, discover_repo_root, monotonic_now, print_report


LINT_NAME = "hermes-atm-boundary"
BOUNDARY_PATH = Path("boundaries/hermes-atm/runtime-composition.toml")
PACKAGE_PATH = Path("crates/hermes-atm/pyproject.toml")
SOURCE_ROOT = Path("crates/hermes-atm/src/hermes_atm")
EXPECTED_RULE = "LINT-BOUNDARY-HERMES-ATM-IMPORTS"
FORBIDDEN_SOURCE_MARKERS = {
    "database_io": ("import sqlite3", "from sqlite3"),
    "direct_socket_io": ("import socket", "from socket"),
    "daemon_lifecycle": ("start_daemon(", "stop_daemon(", "restart_daemon("),
    "retry_queue": ("RetryQueue", "queue_retry(", "retry_queue"),
}
FORBIDDEN_REFERENCES = ("rusqlite", "reqwest")
FORBIDDEN_DEPENDENCIES = ("atm-daemon", "atm-storage-rusqlite")


@dataclass(frozen=True)
class Violation:
    location: str
    message: str

    def render(self) -> str:
        return f"{self.location}: {self.message}"


def load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def source_files(root: Path) -> list[Path]:
    return sorted(root.rglob("*.py"))


def collect_violations(repo_root: Path) -> list[Violation]:
    boundary = load_toml(repo_root / BOUNDARY_PATH)
    package = load_toml(repo_root / PACKAGE_PATH)
    violations: list[Violation] = []
    enforcement = boundary.get("enforcement", {})
    if EXPECTED_RULE not in enforcement.get("lint_rules", []):
        violations.append(Violation(str(BOUNDARY_PATH), f"must declare {EXPECTED_RULE}"))
    forbidden_tags = set(boundary.get("ownership", {}).get("io_forbidden", []))
    expected_tags = set(FORBIDDEN_SOURCE_MARKERS)
    if forbidden_tags != expected_tags:
        violations.append(Violation(str(BOUNDARY_PATH), f"io_forbidden must be exactly {sorted(expected_tags)}"))
    declared_edges = set(boundary.get("dependencies", {}).get("forbidden_edges", []))
    for dependency in FORBIDDEN_DEPENDENCIES:
        edge = f"hermes-atm -> {dependency}"
        if edge not in declared_edges:
            violations.append(Violation(str(BOUNDARY_PATH), f"missing forbidden edge {edge}"))
    declared_references = set(boundary.get("references", {}).get("forbidden", []))
    if declared_references != {"rusqlite::Connection", "reqwest::Client"}:
        violations.append(Violation(str(BOUNDARY_PATH), "forbidden references must cover rusqlite and reqwest"))

    dependencies = package.get("project", {}).get("dependencies", [])
    if dependencies != ["atm-graft>=1.4,<1.5"]:
        violations.append(Violation(str(PACKAGE_PATH), "dependencies must be exactly the public atm-graft 1.4.x contract"))
    package_text = (repo_root / PACKAGE_PATH).read_text(encoding="utf-8").lower()
    for dependency in FORBIDDEN_DEPENDENCIES:
        if dependency in package_text:
            violations.append(Violation(str(PACKAGE_PATH), f"forbidden package edge to {dependency}"))

    for path in source_files(repo_root / SOURCE_ROOT):
        text = path.read_text(encoding="utf-8")
        location = path.relative_to(repo_root).as_posix()
        for tag, markers in FORBIDDEN_SOURCE_MARKERS.items():
            for marker in markers:
                if marker in text:
                    violations.append(Violation(location, f"forbidden {tag} marker {marker!r}"))
        for reference in FORBIDDEN_REFERENCES:
            if reference in text:
                violations.append(Violation(location, f"forbidden reference {reference!r}"))
    return violations


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root")
    args = parser.parse_args(argv[1:])
    root = discover_repo_root(args.root)
    started = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    violations = collect_violations(root)
    findings = [item.render() for item in violations]
    report = build_report(
        lint_name=LINT_NAME, repo_root=root, passed=not violations,
        summary="hermes-atm boundary policy enforced" if not violations else "hermes-atm boundary policy violated",
        findings=findings, transcript_lines=findings or ["all declared hermes-atm boundary rules enforced"],
        started_at=started, duration_seconds=monotonic_now() - started_monotonic,
    )
    print_report(report, repo_root=root, preview_limit=0, direct_threshold=0)
    return 0 if not violations else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
