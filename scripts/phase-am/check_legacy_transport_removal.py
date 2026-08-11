#!/usr/bin/env python3
"""Phase AM negative guard for removed legacy transport surfaces.

The guard is registered in ``just lint``. Its enabled categories correspond to
the legacy transport surfaces deleted by their owning Phase AM sprint.
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
    excluded_paths: tuple[str, ...] = ()


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
    GuardRule("raw-framing", "handwritten HTTP parser or writer", re.compile(r"\b(?:decode_request|read_http_request|read_http_response(?:_with_frame_reader)?|write_http_request(?:_with_headers)?|write_http_response|write_local_http_response)\b"), ("crates/",)),
    GuardRule("peer-ingress", "peer-only ingress protocol", re.compile(r"\b(?:PEER_SOURCE_HOST_HEADER|PeerMessageArray|peer_sync_path_host|normalize_peer_write_for_local_delivery|route_peer_http_request)\b"), ("crates/",)),
    GuardRule("resend-replay", "peer delivery scheduler or replay state", re.compile(r"\b(?:PeerDrainCoordinator|PeerDeliveryCoordinator|PeerDeliveryProjection|PeerDeliveryEvent|PeerRecovery(?:Scheduled|Attempt)|PostCommitWorkKey::PeerDelivery|peer_delivery_observability|OutboundMessageQuery|StoredPeerWrite|SqliteOutboundMessageQuery|build_peer_outbound_replay|page_for_peer)\b"), ("crates/",)),
    GuardRule("direct-sqlite", "direct rusqlite import in daemon/runtime", re.compile(r"^\s*(?:use|extern\s+crate)\s+rusqlite\b"), ("crates/atm-daemon/", "crates/atm-http-runtime/")),
    GuardRule("direct-sqlite", "direct rusqlite dependency in daemon/runtime", re.compile(r"^\s*rusqlite\s*="), ("crates/atm-daemon/", "crates/atm-http-runtime/")),
    GuardRule("daemon-harness", "daemon tmux code reference", re.compile(r"\b(?:tmux_command|run_tmux_command|Tmux)\b"), ("crates/atm-daemon/",), ("crates/atm-daemon/src/message_received_emitter.rs",)),
    GuardRule("daemon-harness", "daemon graft code or dependency", re.compile(r"\b(?:atm_graft|GraftClient|GraftReceiveHook)\b|^\s*atm-graft\s*="), ("crates/atm-daemon/",)),
    GuardRule("dead-daemon-dispatch", "retired daemon request dispatcher", re.compile(r"\bDaemonRequestDispatcher\b"), ("crates/atm-daemon/",)),
    GuardRule("dead-daemon-dispatch", "retired daemon write dispatcher seam", re.compile(r"\b(?:MessageWriter|PostWriteRouter|run_received_hook)\b"), ("crates/atm-daemon/",)),
)


def iter_production_sources(repo_root: Path) -> tuple[Path, ...]:
    sources: list[Path] = []
    crates = repo_root / "crates"
    if not crates.exists():
        return ()
    for path in sorted(crates.rglob("*.rs")):
        relative = path.relative_to(repo_root).as_posix()
        if "/tests/" not in relative and "/test_support" not in relative and "-test-support/" not in relative and not path.name.endswith("_tests.rs"):
            sources.append(path)
    for path in sorted(crates.glob("*/Cargo.toml")):
        sources.append(path)
    return tuple(sources)


def rules_for_categories(categories: tuple[str, ...]) -> tuple[GuardRule, ...]:
    known = {rule.category for rule in RULES}
    unknown = sorted(set(categories) - known)
    if unknown:
        raise ValueError(f"unknown Phase AM guard categories: {', '.join(unknown)}")
    return tuple(rule for rule in RULES if not categories or rule.category in categories)


def is_code_line(line: str) -> bool:
    return not line.lstrip().startswith(("//", "#"))


def find_violations(repo_root: Path, rules: tuple[GuardRule, ...] = RULES) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    for path in iter_production_sources(repo_root):
        relative = path.relative_to(repo_root).as_posix()
        text = path.read_text(encoding="utf-8")
        for line_number, line in enumerate(text.splitlines(), start=1):
            if not is_code_line(line):
                continue
            for rule in rules:
                if relative.startswith(rule.path_prefixes) and relative not in rule.excluded_paths and rule.pattern.search(line):
                    violations.append(Violation(Path(relative), line_number, rule.category, rule.label, line.strip()))
    return tuple(violations)


def main() -> int:
    parser = argparse.ArgumentParser(description="Draft Phase AM legacy-transport removal guard")
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument(
        "--category",
        action="append",
        choices=sorted({rule.category for rule in RULES}),
        help="Enable only one deletion category; repeat to enable multiple categories.",
    )
    args = parser.parse_args()
    categories = tuple(args.category or ())
    violations = find_violations(args.repo_root.resolve(), rules_for_categories(categories))
    if violations:
        print("phase-am legacy transport removal guard failed")
        print("\n".join(violation.render() for violation in violations))
        return 1
    print("phase-am legacy transport removal guard passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
