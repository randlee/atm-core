#!/usr/bin/env python3
"""Draft-only Phase AM negative guard for removed legacy transport surfaces.

This module is intentionally not registered in ``just lint`` during AM.1:
the listed production symbols remain live until their owning deletion sprint.
AM.2--AM.5 enable the applicable categories in the same change that removes
their live references.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import re


@dataclass(frozen=True)
class GuardRule:
    category: str
    label: str
    pattern: re.Pattern[str]
    path_prefixes: tuple[str, ...]


@dataclass(frozen=True)
class Violation:
    path: Path
    line_number: int
    category: str
    label: str
    line: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: [{self.category}] {self.label}: {self.line}"


RULES = (
    GuardRule("raw-framing", "handwritten HTTP frame reader", re.compile(r"\bHttpFrameReader\b"), ("crates/",)),
    GuardRule("raw-framing", "handwritten HTTP request writer", re.compile(r"\bwrite_http_request(?:_with_headers)?\b"), ("crates/",)),
    GuardRule("peer-ingress", "peer-only provenance header", re.compile(r"\bPEER_SOURCE_HOST_HEADER\b"), ("crates/",)),
    GuardRule("peer-ingress", "peer-only request grammar", re.compile(r"\bPeerMessageArray\b"), ("crates/",)),
    GuardRule("resend-replay", "peer drain coordinator", re.compile(r"\bPeerDrainCoordinator\b"), ("crates/",)),
    GuardRule("resend-replay", "peer delivery coordinator", re.compile(r"\bPeerDeliveryCoordinator\b"), ("crates/",)),
    GuardRule("direct-sqlite", "direct rusqlite import in daemon/runtime", re.compile(r"\brusqlite\b"), ("crates/atm-daemon/", "crates/atm-http-runtime/")),
    GuardRule("daemon-harness", "daemon tmux reference", re.compile(r"\btmux\b"), ("crates/atm-daemon/",)),
    GuardRule("daemon-harness", "daemon graft reference", re.compile(r"\batm_graft\b|\batm-graft\b"), ("crates/atm-daemon/",)),
)


def iter_production_sources(repo_root: Path) -> tuple[Path, ...]:
    sources: list[Path] = []
    crates = repo_root / "crates"
    if not crates.exists():
        return ()
    for path in sorted(crates.rglob("*.rs")):
        relative = path.relative_to(repo_root).as_posix()
        if "/tests/" not in relative and "/test_support" not in relative and not path.name.endswith("_tests.rs"):
            sources.append(path)
    for path in sorted(crates.glob("*/Cargo.toml")):
        sources.append(path)
    return tuple(sources)


def find_violations(repo_root: Path, rules: tuple[GuardRule, ...] = RULES) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    for path in iter_production_sources(repo_root):
        relative = path.relative_to(repo_root).as_posix()
        text = path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            for rule in rules:
                if relative.startswith(rule.path_prefixes) and rule.pattern.search(line):
                    violations.append(Violation(Path(relative), line_number, rule.category, rule.label, line.strip()))
    return tuple(violations)


def main() -> int:
    parser = argparse.ArgumentParser(description="Draft Phase AM legacy-transport removal guard")
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    violations = find_violations(args.repo_root.resolve())
    if violations:
        print("phase-am legacy transport removal guard failed")
        print("\n".join(violation.render() for violation in violations))
        return 1
    print("phase-am legacy transport removal guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
