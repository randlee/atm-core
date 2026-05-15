#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
import argparse
import re
import sys


@dataclass(frozen=True)
class Violation:
    path: Path
    line_number: int
    label: str
    line: str


@dataclass(frozen=True)
class AllowedLiteral:
    path_suffix: str
    line_pattern: re.Pattern[str]


FORBIDDEN_PATTERNS = (
    ("legacy mailbox runtime type", re.compile(r"\bLegacyMailboxRuntime\b")),
    ("legacy mailbox runtime enum branch", re.compile(r"\bDefaultMailboxRuntime::Legacy\b")),
    ("legacy runtime helper", re.compile(r"\blegacy_runtime\s*\(")),
    ("legacy mailbox mode helper", re.compile(r"\ballows_legacy_mailbox_files\s*\(")),
    ("source-file mailbox read helper", re.compile(r"\bread_messages\s*\(")),
    ("source-file mailbox observe helper", re.compile(r"\bobserve_source_files\s*\(")),
    ("source-file mailbox commit helper", re.compile(r"\bcommit_source_files\s*\(")),
    ("source-file mailbox state helper", re.compile(r"\bcommit_mailbox_state\s*\(")),
    ("source-file mailbox lock helper", re.compile(r"\bwith_locked_source_files\s*\(")),
)

ALLOWED_LEGACY_LITERALS = (
    AllowedLiteral(
        "crates/atm-core/src/boundary_support.rs",
        # Boundary-owned reexport of store-backed state to the Claude inbox
        # surface still delegates through the compatibility projection writer.
        re.compile(r"mailbox::store::commit_mailbox_state\(&request\.path, &request\.messages\)\?;"),
    ),
    AllowedLiteral(
        "crates/atm-core/src/list.rs",
        # Production list/read code still rejects compatibility-only message
        # keys by checking for the historical `legacy:` prefix explicitly.
        re.compile(r'message_key\.as_ref\(\)\.starts_with\("legacy:"\)'),
    ),
    AllowedLiteral(
        "crates/atm-core/src/mailbox/mod.rs",
        # Test-only mailbox helpers document and exercise the compatibility
        # projection writer without exposing a second production runtime path.
        re.compile(r"mailbox::store::commit_mailbox_state\(\)|store::commit_mailbox_state\(path, &messages\)"),
    ),
    AllowedLiteral(
        "crates/atm-core/src/mailbox/store.rs",
        # The mailbox store module owns the compatibility projection writer
        # definition and its unit tests for the Claude-owned inbox surface.
        re.compile(r"\bcommit_mailbox_state\b"),
    ),
    AllowedLiteral(
        "crates/atm-core/src/read/mod.rs",
        # Read-path compatibility filtering uses the same explicit legacy-key
        # prefix check while the old mailbox runtime is being deleted.
        re.compile(r'message_key\.as_ref\(\)\.starts_with\("legacy:"\)'),
    ),
    AllowedLiteral(
        "crates/atm-core/src/send/mod.rs",
        # Send persists store-backed state first, then rewrites the shared
        # inbox compatibility projection for Claude-owned consumers.
        re.compile(r"crate::mailbox::store::commit_mailbox_state\(inbox_path, &inbox_messages\)\?;"),
    ),
    AllowedLiteral(
        "crates/atm-core/src/workflow.rs",
        # Workflow import/export still documents and parses historical
        # compatibility ids during migration-state validation and tests.
        re.compile(r"`legacy:` remains|must start with 'atm:' or 'legacy:'|strip_prefix\(\"legacy:\"\)|legacy:01"),
    ),
)


def iter_rust_sources(repo_root: Path) -> tuple[Path, ...]:
    crates_dir = repo_root / "crates"
    return tuple(sorted(crates_dir.rglob("*.rs")))


def is_allowed_line(relative_path: str, line: str) -> bool:
    return any(
        relative_path.endswith(allowed.path_suffix) and allowed.line_pattern.search(line)
        for allowed in ALLOWED_LEGACY_LITERALS
    )


def is_allowed_legacy_literal(relative_path: str, line: str) -> bool:
    return is_allowed_line(relative_path, line)


def find_violations(repo_root: Path) -> tuple[Violation, ...]:
    violations: list[Violation] = []
    for path in iter_rust_sources(repo_root):
        relative_path = path.relative_to(repo_root).as_posix()
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            for label, pattern in FORBIDDEN_PATTERNS:
                if pattern.search(line) and not is_allowed_line(relative_path, line):
                    violations.append(Violation(Path(relative_path), line_number, label, line.strip()))
            if "legacy:" in line and not is_allowed_line(relative_path, line):
                violations.append(
                    Violation(
                        Path(relative_path),
                        line_number,
                        "unexpected production legacy mailbox literal",
                        line.strip(),
                    )
                )
    return tuple(violations)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Enforce the Phase X legacy-mailbox deletion regression gate on workspace Rust sources."
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="Workspace root to scan. Defaults to the repository that owns this script.",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    repo_root = args.repo_root.resolve()
    violations = find_violations(repo_root)
    if violations:
        print("legacy-mailbox-paths failed")
        for violation in violations:
            print(
                f"{violation.path}:{violation.line_number}: {violation.label}: {violation.line}"
            )
        return 1

    print("legacy-mailbox-paths passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
