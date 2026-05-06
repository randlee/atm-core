#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import argparse
import re
import sys
import time
import tomllib


JUST_DIR = Path(__file__).resolve().parents[1] / ".just"
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_common import build_report
from lint_common import discover_repo_root
from lint_common import monotonic_now
from lint_common import print_report
from lint_common import workspace_crate_section_lines


LINT_NAME = "daemon-singleton"
DEFAULT_CONFIG_PATH = Path("scripts/lint_daemon_singleton.toml")
CFG_TEST_LINE_RE = re.compile(r"(?m)^\s*#\[\s*cfg\s*\(\s*test\s*\)\s*\]")

PROHIBITED_CATEGORY_DESCRIPTIONS = {
    "spawn_test_daemon": "spawn_test_daemon helper in ordinary tests",
    "warm_daemon": "warm_daemon helper in ordinary tests",
    "daemon_guard": "DaemonGuard daemon lifecycle helper in ordinary tests",
    "atm_daemon_bin": "ATM_DAEMON_BIN usage in ordinary tests",
    "daemon_socket_path": "atm-daemon.sock path construction in ordinary tests",
    "direct_atm_daemon_command": "direct atm-daemon Command::new spawn path",
    "timing_warmup_shortcut": "timing-based daemon warmup shortcut",
}

PROHIBITED_PATTERNS = {
    "spawn_test_daemon": re.compile(r"\bspawn_test_daemon\b"),
    "warm_daemon": re.compile(r"\bwarm_daemon\b"),
    "daemon_guard": re.compile(r"\bDaemonGuard\b"),
    "atm_daemon_bin": re.compile(r"\bATM_DAEMON_BIN\b"),
    "daemon_socket_path": re.compile(r"atm-daemon\.sock"),
}

SLEEP_PATTERN = re.compile(r"\b(?:std::thread::sleep|thread::sleep)\s*\(")
DAEMON_TIMING_ANCHORS = (
    "warm_daemon",
    "spawn_test_daemon",
    "is_daemon_start_transient",
    "daemon warmup",
    "atm-daemon.sock",
    "ATM_DAEMON_BIN",
)
UNIX_GATING_PATTERNS = (
    "#[cfg(unix)]",
    "#[cfg(all(unix",
    "#[cfg(any(unix",
    "cfg!(unix)",
)


@dataclass(frozen=True)
class AllowEntry:
    path: str
    categories: tuple[str, ...]
    reason: str
    require_unix_gating: bool = False


@dataclass(frozen=True)
class Violation:
    path: str
    line_number: int
    category: str
    detail: str
    unix_gated: bool = False

    def render(self) -> str:
        return f"{self.path}:{self.line_number}: [{self.category}] {self.detail}"


@dataclass(frozen=True)
class SourceScope:
    source: str
    base_line_number: int


def load_allow_entries(repo_root: Path, config_path: Path) -> list[AllowEntry]:
    absolute_config = config_path if config_path.is_absolute() else repo_root / config_path
    if not absolute_config.exists():
        return []
    parsed = tomllib.loads(absolute_config.read_text(encoding="utf-8"))
    singleton = parsed.get("daemon_singleton", {})
    if not isinstance(singleton, dict):
        return []

    entries = singleton.get("allow", [])
    if not isinstance(entries, list):
        return []

    allow_entries: list[AllowEntry] = []
    seen_keys: set[tuple[str, str]] = set()
    for entry in entries:
        if not isinstance(entry, dict):
            continue
        path = entry.get("path")
        categories = entry.get("categories")
        reason = entry.get("reason")
        require_unix_gating = entry.get("require_unix_gating", False)
        if not isinstance(path, str) or not isinstance(reason, str):
            continue
        if not isinstance(categories, list) or not all(isinstance(item, str) for item in categories):
            continue
        for category in categories:
            key = (path, category)
            if key in seen_keys:
                raise ValueError(
                    f"duplicate daemon_singleton allow entry for {path} [{category}] in {absolute_config.as_posix()}"
                )
            seen_keys.add(key)
        allow_entries.append(
            AllowEntry(
                path=path,
                categories=tuple(categories),
                reason=reason,
                require_unix_gating=bool(require_unix_gating),
            )
        )
    return allow_entries


def is_test_code(path: Path, source: str) -> bool:
    if "tests" in path.parts:
        return True
    return bool(CFG_TEST_LINE_RE.search(source))


def rust_sources(repo_root: Path) -> list[Path]:
    return sorted(path for path in repo_root.glob("crates/**/*.rs") if path.is_file())


def line_number_for_offset(source: str, offset: int) -> int:
    return source.count("\n", 0, offset) + 1


def line_offsets(source: str) -> list[int]:
    offsets: list[int] = []
    offset = 0
    for line in source.splitlines(keepends=True):
        offsets.append(offset)
        offset += len(line)
    return offsets


def find_cfg_test_scopes(source: str) -> list[SourceScope]:
    scopes: list[SourceScope] = []
    offset = 0
    marker = "#[cfg(test)]"
    while True:
        marker_index = source.find(marker, offset)
        if marker_index == -1:
            break
        brace_index = source.find("{", marker_index)
        if brace_index == -1:
            break
        # This brace walk is intentionally lightweight and does not attempt
        # string/comment-aware Rust parsing. It is sufficient for the current
        # `#[cfg(test)] mod tests { ... }` shapes but may miss pathological
        # cases where brace-like text appears before the matching module close.
        depth = 0
        end_index = brace_index
        while end_index < len(source):
            character = source[end_index]
            if character == "{":
                depth += 1
            elif character == "}":
                depth -= 1
                if depth == 0:
                    end_index += 1
                    break
            end_index += 1
        if depth != 0:
            break
        scopes.append(
            SourceScope(
                source=source[marker_index:end_index],
                base_line_number=line_number_for_offset(source, marker_index) - 1,
            )
        )
        offset = end_index
    return scopes


def test_scopes(path: Path, source: str) -> list[SourceScope]:
    if "tests" in path.parts:
        return [SourceScope(source=source, base_line_number=0)]
    return find_cfg_test_scopes(source)


def header_start_for_open(source: str, open_index: int) -> int:
    header_start = source.rfind("\n", 0, open_index) + 1
    while header_start > 0:
        previous_line_end = header_start - 1
        previous_line_start = source.rfind("\n", 0, previous_line_end) + 1
        previous_line = source[previous_line_start:previous_line_end].strip()
        if previous_line.startswith("#["):
            header_start = previous_line_start
            continue
        break
    return header_start


def iter_block_ranges(source: str) -> list[tuple[int, int]]:
    blocks: list[tuple[int, int]] = []
    stack: list[int] = []
    for index, character in enumerate(source):
        if character == "{":
            stack.append(index)
        elif character == "}":
            if not stack:
                continue
            open_index = stack.pop()
            blocks.append((header_start_for_open(source, open_index), index + 1))
    return sorted(blocks, key=lambda item: (item[0], item[1] - item[0]))


def block_sources_for_offset(source: str, offset: int) -> list[str]:
    matching_ranges = [(start, end) for start, end in iter_block_ranges(source) if start <= offset < end]
    matching_ranges.sort(key=lambda item: item[1] - item[0])
    return [source[start:end] for start, end in matching_ranges]


def unix_gating_for_offset(source: str, offset: int) -> bool:
    return any(has_unix_gating(block_source) for block_source in block_sources_for_offset(source, offset))


def find_direct_atm_daemon_commands(relative_path: str, scope: SourceScope) -> list[Violation]:
    violations: list[Violation] = []
    lines = scope.source.splitlines()
    offsets = line_offsets(scope.source)
    for index, line in enumerate(lines):
        if "Command::new(" not in line:
            continue
        # We intentionally scan only a short forward window because current
        # launcher patterns keep the resolved daemon path near the callsite.
        # If the relevant trigger moves farther away than eight lines, this
        # detector will miss it; the gap is accepted to avoid a much noisier
        # whole-function search.
        window = "\n".join(lines[index : min(index + 8, len(lines))])
        if "atm-daemon" in window or "test_daemon_launcher" in window:
            offset = offsets[index]
            violations.append(
                Violation(
                    path=relative_path,
                    line_number=scope.base_line_number + index + 1,
                    category="direct_atm_daemon_command",
                    detail=PROHIBITED_CATEGORY_DESCRIPTIONS["direct_atm_daemon_command"],
                    unix_gated=unix_gating_for_offset(scope.source, offset),
                )
            )
    return violations


def find_timing_warmup_shortcuts(relative_path: str, scope: SourceScope) -> list[Violation]:
    violations: list[Violation] = []
    lines = scope.source.splitlines()
    offsets = line_offsets(scope.source)
    for index, line in enumerate(lines):
        if not SLEEP_PATTERN.search(line):
            continue
        context_start = max(0, index - 8)
        context_end = min(len(lines), index + 9)
        window = "\n".join(lines[context_start:context_end])
        if any(anchor in window for anchor in DAEMON_TIMING_ANCHORS):
            offset = offsets[index]
            violations.append(
                Violation(
                    path=relative_path,
                    line_number=scope.base_line_number + index + 1,
                    category="timing_warmup_shortcut",
                    detail=PROHIBITED_CATEGORY_DESCRIPTIONS["timing_warmup_shortcut"],
                    unix_gated=unix_gating_for_offset(scope.source, offset),
                )
            )
    return violations


def collect_violations(repo_root: Path) -> list[Violation]:
    violations: list[Violation] = []
    for path in rust_sources(repo_root):
        source = path.read_text(encoding="utf-8")
        if not is_test_code(path, source):
            continue
        relative_path = path.relative_to(repo_root).as_posix()
        for scope in test_scopes(path, source):
            for category, pattern in PROHIBITED_PATTERNS.items():
                for match in pattern.finditer(scope.source):
                    match_offset = match.start()
                    violations.append(
                        Violation(
                            path=relative_path,
                            line_number=scope.base_line_number + line_number_for_offset(scope.source, match_offset),
                            category=category,
                            detail=PROHIBITED_CATEGORY_DESCRIPTIONS[category],
                            unix_gated=unix_gating_for_offset(scope.source, match_offset),
                        )
                    )

            violations.extend(find_direct_atm_daemon_commands(relative_path, scope))
            violations.extend(find_timing_warmup_shortcuts(relative_path, scope))

    return sorted(violations, key=lambda item: (item.path, item.line_number, item.category))


def has_unix_gating(source: str) -> bool:
    return any(pattern in source for pattern in UNIX_GATING_PATTERNS)


def apply_allow_entries(
    violations: list[Violation],
    allow_entries: list[AllowEntry],
) -> tuple[list[Violation], list[str]]:
    allowed_messages: list[str] = []
    remaining: list[Violation] = []
    allow_by_key = {(entry.path, category): entry for entry in allow_entries for category in entry.categories}

    for violation in violations:
        entry = allow_by_key.get((violation.path, violation.category))
        if entry is None:
            remaining.append(violation)
            continue

        if entry.require_unix_gating:
            if not violation.unix_gated:
                remaining.append(
                    Violation(
                        path=violation.path,
                        line_number=violation.line_number,
                        category=violation.category,
                        detail=f"{violation.detail}; allow-list entry requires explicit #[cfg(unix)] gating",
                        unix_gated=False,
                    )
                )
                continue

        allowed_messages.append(
            f"allowed {violation.path} [{violation.category}] — {entry.reason}"
        )

    return remaining, allowed_messages


def build_summary(violations: list[Violation], allow_entries: list[AllowEntry], allowed_count: int) -> str:
    if not violations:
        return f"daemon singleton policy satisfied ({allowed_count} allow-listed match(es), {len(allow_entries)} allow entry/entries)"
    return (
        f"daemon singleton policy violated ({len(violations)} finding(s), "
        f"{allowed_count} allow-listed match(es), {len(allow_entries)} allow entry/entries)"
    )


def run(repo_root: Path, config_path: Path) -> int:
    started_at = datetime.now(timezone.utc)
    started_monotonic = monotonic_now()
    try:
        allow_entries = load_allow_entries(repo_root, config_path)
    except ValueError as error:
        duration_seconds = monotonic_now() - started_monotonic
        report = build_report(
            lint_name=LINT_NAME,
            repo_root=repo_root,
            passed=False,
            summary="daemon singleton policy configuration is invalid",
            findings=[str(error)],
            transcript_lines=[
                f"config: {(config_path if config_path.is_absolute() else config_path).as_posix()}",
                str(error),
            ],
            started_at=started_at,
            duration_seconds=duration_seconds,
        )
        print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
        return 1
    violations = collect_violations(repo_root)
    filtered_violations, allowed_messages = apply_allow_entries(violations, allow_entries)
    duration_seconds = monotonic_now() - started_monotonic

    findings = [violation.render() for violation in filtered_violations]
    transcript_lines = workspace_crate_section_lines(repo_root)
    transcript_lines.append(f"config: {(config_path if config_path.is_absolute() else config_path).as_posix()}")
    if allow_entries:
        transcript_lines.append(f"allow entries: {len(allow_entries)}")
    else:
        transcript_lines.append("allow entries: 0 (explicitly no exceptions)")
    transcript_lines.append("")
    if allowed_messages:
        transcript_lines.append("allowed matches:")
        transcript_lines.extend(allowed_messages)
        transcript_lines.append("")
    transcript_lines.append("violations:")
    transcript_lines.extend(findings or ["no daemon-singleton violations found"])

    report = build_report(
        lint_name=LINT_NAME,
        repo_root=repo_root,
        passed=not filtered_violations,
        summary=build_summary(filtered_violations, allow_entries, len(allowed_messages)),
        findings=findings,
        transcript_lines=transcript_lines,
        started_at=started_at,
        duration_seconds=duration_seconds,
    )
    print_report(report, repo_root=repo_root, preview_limit=4, direct_threshold=4)
    return 0 if report.passed else 1


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Enforce the daemon singleton/no-spawn rule for ATM test code."
    )
    parser.add_argument("--root", help="Repo root to inspect.")
    parser.add_argument(
        "--config",
        default=str(DEFAULT_CONFIG_PATH),
        help="Path to the daemon-singleton allow-list TOML.",
    )
    args = parser.parse_args(argv[1:])
    repo_root = discover_repo_root(args.root)
    config_path = Path(args.config)
    return run(repo_root, config_path)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
