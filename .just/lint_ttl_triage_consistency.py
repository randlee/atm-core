#!/usr/bin/env python3
from __future__ import annotations

import argparse
from fnmatch import fnmatch
import re
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report


LINT_NAME = "ttl-triage"
STATE_RE = re.compile(
    r"triage:(?P<field>status|findingAggregate|branchStatus|branchR[A-Za-z0-9_.-]+Status)\s+"
    r"(?:\"(?P<quoted>[^\"]+)\"|triage:(?P<qualified>[A-Za-z0-9_.-]+))"
)
SPRINT_FIELD_RE = re.compile(
    r"^\s*(?P<predicate>triage:(?:foundIn|aich_sprint|aichSprint|sprint_id|sprint))\s+"
    r"(?:triage:)?(?:\"(?P<quoted>[^\"]+)\"|(?P<bare>[A-Za-z0-9_.-]+))"
)
CANONICAL_SPRINT_RE = re.compile(r"^(?P<phase>[A-Za-z][A-Za-z0-9]*)\.(?P<number>[1-9][0-9]*)$")
LEGACY_DASH_SPRINT_RE = re.compile(
    r"^(?P<phase>[A-Za-z][A-Za-z0-9]*)-S(?P<number>[0-9]+)$", re.IGNORECASE
)
LEGACY_COMPACT_SPRINT_RE = re.compile(r"^(?P<phase>[A-Za-z][A-Za-z0-9]*?)(?P<number>[0-9]+)$")
SPRINT_LIKE_RE = re.compile(r"^[A-Za-z]{2}(?:\.\d+|-S?\d+)$", re.IGNORECASE)


@dataclass(frozen=True)
class TriageConsistencyViolation:
    path: str
    line_number: int
    message: str

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: {self.message}"


def iter_ttl_files(repo_root: Path) -> list[Path]:
    # This validator is intentionally scoped to repo-owned triage records. It
    # does not walk arbitrary Turtle assets outside `.triage/`.
    triage_root = repo_root / ".triage"
    if not triage_root.exists():
        return []
    return sorted(triage_root.rglob("*.ttl"))


def load_legacy_allowlist(repo_root: Path) -> list[tuple[str, str]]:
    allowlist_path = repo_root / ".just/ttl-naming-legacy-allowlist.txt"
    if not allowlist_path.exists():
        return []
    entries: list[tuple[str, str]] = []
    for raw_line in allowlist_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        path_glob, separator, raw_value = line.partition("\t")
        if separator and path_glob and raw_value:
            entries.append((path_glob, raw_value))
    return entries


def is_allowlisted_legacy(allowlist: list[tuple[str, str]], path: str, raw_value: str) -> bool:
    return any(
        fnmatch(path, path_glob) and raw_value == allowed_value
        for path_glob, allowed_value in allowlist
    )


def extract_finding_block(text: str) -> tuple[list[str], int] | None:
    # This validator intentionally assumes one `triage:Finding` block per TTL
    # file because `.triage/phase-R/findings/*.ttl` is a one-finding-per-file
    # repository convention. If that convention changes, this extractor needs
    # to become multi-block aware instead of silently scanning only the first.
    lines = text.splitlines()
    for index, line in enumerate(lines):
        if "a triage:Finding" not in line:
            continue
        start = index
        while start > 0 and lines[start - 1].strip():
            start -= 1
        end = index + 1
        while end < len(lines) and lines[end].strip():
            end += 1
        return lines[start:end], start + 1
    return None


def normalize_state(match: re.Match[str]) -> str:
    value = match.group("quoted") or match.group("qualified") or ""
    return value.strip().lower()


def canonical_sprint_id(raw_value: str) -> tuple[str | None, str]:
    """Return the canonical sprint key and its diagnostic class.

    Comparison is deliberately case-insensitive, but persistence is not: a
    canonical match with different casing is reported so ingestion callers can
    normalize before writing.  Legacy separators and compact keys are mapped
    only when the phase/number split is unambiguous.
    """

    value = raw_value.strip()
    match = CANONICAL_SPRINT_RE.fullmatch(value)
    if match:
        phase = match.group("phase").upper()
        number = str(int(match.group("number")))
        canonical = f"{phase}.{number}"
        return canonical, "ok" if value == canonical else "NAMING.NON_CANONICAL"

    match = LEGACY_DASH_SPRINT_RE.fullmatch(value)
    if match:
        return (
            f"{match.group('phase').upper()}.{int(match.group('number'))}",
            "TTL.QA_RUN_KEY_MISMATCH",
        )

    match = LEGACY_COMPACT_SPRINT_RE.fullmatch(value)
    if match:
        return f"{match.group('phase').upper()}.{int(match.group('number'))}", "NAMING.LEGACY_IDENTIFIER"

    return None, "NAMING.UNKNOWN_SPRINT_FORMAT"


def collect_ttl_triage_violations(repo_root: Path) -> list[TriageConsistencyViolation]:
    violations: list[TriageConsistencyViolation] = []
    legacy_allowlist = load_legacy_allowlist(repo_root)
    for ttl_path in iter_ttl_files(repo_root):
        rel_path = ttl_path.relative_to(repo_root).as_posix()
        finding_block = extract_finding_block(ttl_path.read_text(encoding="utf-8"))
        if finding_block is None:
            continue
        lines, start_line = finding_block
        top_statuses: list[tuple[str, int]] = []
        aggregate_statuses: list[tuple[str, int]] = []
        branch_statuses: list[tuple[str, int]] = []

        for offset, line in enumerate(lines, start=start_line):
            for match in SPRINT_FIELD_RE.finditer(line):
                raw_value = match.group("quoted") or match.group("bare") or ""
                if match.group("predicate") == "triage:foundIn" and not SPRINT_LIKE_RE.fullmatch(
                    raw_value.strip()
                ):
                    # `foundIn` also names non-sprint artifacts (for example
                    # HERMES-SMOKE-QA-1). Only sprint-shaped values participate
                    # in the naming contract; explicit sprint fields are
                    # always validated below.
                    continue
                canonical, diagnostic = canonical_sprint_id(raw_value)
                if diagnostic == "ok":
                    continue
                if is_allowlisted_legacy(legacy_allowlist, rel_path, raw_value.strip()):
                    continue
                candidate = f"; canonical candidate={canonical}" if canonical else ""
                violations.append(
                    TriageConsistencyViolation(
                        path=rel_path,
                        line_number=offset,
                        message=(
                            f"{diagnostic}: {match.group('predicate')} value {raw_value!r} "
                            f"is not persisted in canonical sprint form{candidate}; "
                            "normalize at ingestion and retain the raw value in diagnostics"
                        ),
                    )
                )

        for offset, line in enumerate(lines, start=start_line):
            for match in STATE_RE.finditer(line):
                state = normalize_state(match)
                field = match.group("field")
                if field == "status":
                    top_statuses.append((state, offset))
                elif field == "findingAggregate":
                    aggregate_statuses.append((state, offset))
                else:
                    branch_statuses.append((state, offset))

        if top_statuses and aggregate_statuses:
            top_state, top_line = top_statuses[0]
            aggregate_state, aggregate_line = aggregate_statuses[0]
            if top_state != aggregate_state:
                violations.append(
                    TriageConsistencyViolation(
                        path=rel_path,
                        line_number=aggregate_line,
                        message=(
                            f"contradictory triage status fields: status={top_state} "
                            f"but findingAggregate={aggregate_state}"
                        ),
                    )
                )

        resolved_state = None
        resolved_line = None
        if aggregate_statuses:
            resolved_state, resolved_line = aggregate_statuses[0]
        elif top_statuses:
            resolved_state, resolved_line = top_statuses[0]
        if resolved_state in {"fixed", "absent"}:
            for branch_state, branch_line in branch_statuses:
                if branch_state == "open":
                    violations.append(
                        TriageConsistencyViolation(
                            path=rel_path,
                            line_number=branch_line,
                            message=(
                                f"contradictory triage branch state: aggregate={resolved_state} "
                                "but branch status remains open"
                            ),
                        )
                    )
                    break
    return violations


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Reject contradictory aggregate and branch status state in triage Turtle records."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    return parser.parse_args(argv[1:])


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    repo_root = discover_repo_root(args.root)
    started_at = datetime.now(timezone.utc)
    start_time = monotonic_now()
    violations = collect_ttl_triage_violations(repo_root)
    duration_seconds = monotonic_now() - start_time

    transcript_lines = [
        "triage files analyzed:",
        *([path.relative_to(repo_root).as_posix() for path in iter_ttl_files(repo_root)] or ["none"]),
        "",
        "findings:",
        *([violation.render() for violation in violations] or ["none"]),
    ]
    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not violations,
        summary=(
            "contradictory triage Turtle state found"
            if violations
            else "no contradictory triage Turtle state found"
        ),
        findings=[violation.render() for violation in violations],
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=3, direct_threshold=3)
    return 0 if report.passed else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
