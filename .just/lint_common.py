#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import re
import time
import tomllib


LOG_DIR = Path(".just/logs")
CONFIG_PATH = Path(".just/lint-config.toml")
TIMESTAMP_FORMAT = "%Y%m%d%H%M%S"
LINT_NAME_RE = re.compile(r"[^A-Za-z0-9._-]+")
DIRECTIVE_RE = re.compile(
    r"(?P<label>(?:lint-[A-Za-z0-9._-]+|rule-\d{3}|lint-all))\s*:\s*"
    r"(?P<action>allow-next-line|allow-start|allow-end)\b",
    re.IGNORECASE,
)


@dataclass(frozen=True)
class LintReport:
    lint_name: str
    passed: bool
    summary: str
    findings: list[str]
    transcript: list[str]
    duration_seconds: float
    log_path: Path


@dataclass(frozen=True)
class LintDirectivePolicy:
    tool_key: str
    aliases: tuple[str, ...] = ()

    @property
    def labels(self) -> set[str]:
        labels = {"lint-all", f"lint-{self.tool_key.lower()}"}
        labels.update(alias.lower() for alias in self.aliases)
        return labels


def discover_repo_root(explicit_root: str | None = None) -> Path:
    if explicit_root is not None:
        return Path(explicit_root).resolve()
    return Path(__file__).resolve().parent.parent


def load_lint_config(repo_root: Path) -> dict:
    config_path = repo_root / CONFIG_PATH
    return tomllib.loads(config_path.read_text(encoding="utf-8"))


def lint_slug(lint_name: str) -> str:
    slug = LINT_NAME_RE.sub("-", lint_name.strip().lower()).strip("-")
    return slug or "lint"


def make_log_path(repo_root: Path, lint_name: str, started_at: datetime | None = None) -> Path:
    timestamp = (started_at or datetime.now(timezone.utc)).strftime(TIMESTAMP_FORMAT)
    return repo_root / LOG_DIR / f"{timestamp}-{lint_slug(lint_name)}.log"


def write_log(log_path: Path, transcript: list[str]) -> None:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    text = "\n".join(transcript).rstrip()
    if text:
        text += "\n"
    log_path.write_text(text, encoding="utf-8")


def format_duration(duration_seconds: float) -> str:
    if duration_seconds < 1:
        return f"{duration_seconds:.2f}s"
    return f"{duration_seconds:.1f}s"


def relative_log_path(repo_root: Path, log_path: Path) -> str:
    try:
        return log_path.relative_to(repo_root).as_posix()
    except ValueError:
        return log_path.as_posix()


def build_transcript_header(
    *,
    lint_name: str,
    repo_root: Path,
    started_at: datetime,
    duration_seconds: float,
    summary: str,
) -> list[str]:
    return [
        f"lint: {lint_name}",
        f"repo_root: {repo_root}",
        f"started_at_utc: {started_at.isoformat()}",
        f"duration: {format_duration(duration_seconds)}",
        f"summary: {summary}",
        "",
    ]


def build_report(
    *,
    lint_name: str,
    repo_root: Path,
    passed: bool,
    summary: str,
    findings: list[str],
    transcript_lines: list[str],
    started_at: datetime,
    duration_seconds: float,
) -> LintReport:
    log_path = make_log_path(repo_root, lint_name, started_at)
    transcript = build_transcript_header(
        lint_name=lint_name,
        repo_root=repo_root,
        started_at=started_at,
        duration_seconds=duration_seconds,
        summary=summary,
    )
    transcript.extend(transcript_lines)
    write_log(log_path, transcript)
    return LintReport(
        lint_name=lint_name,
        passed=passed,
        summary=summary,
        findings=findings,
        transcript=transcript,
        duration_seconds=duration_seconds,
        log_path=log_path,
    )


def print_report(
    report: LintReport,
    *,
    repo_root: Path,
    preview_limit: int = 2,
    direct_threshold: int = 3,
) -> None:
    if report.passed:
        print(f"{report.lint_name} passed [{format_duration(report.duration_seconds)}]")
        return

    print(f"{report.lint_name} failed")
    preview = report.findings[:preview_limit]
    if len(report.findings) <= direct_threshold:
        preview = report.findings

    for finding in preview:
        print(f"  {finding}")

    log_display = relative_log_path(repo_root, report.log_path)
    if len(report.findings) > direct_threshold:
        print(f"  [{len(report.findings)}] errors in {log_display}")
    else:
        print(f"  full log: {log_display}")


def monotonic_now() -> float:
    return time.perf_counter()


def is_comment_line(line: str) -> bool:
    stripped = line.strip()
    return stripped.startswith(("//", "///", "//!", "/*", "*", "*/"))


def is_comment_or_empty(line: str) -> bool:
    stripped = line.strip()
    if not stripped:
        return True
    return is_comment_line(line)


def is_code_line(line: str) -> bool:
    return not is_comment_or_empty(line)


def matching_directive_actions(line: str, policy: LintDirectivePolicy) -> list[str]:
    actions: list[str] = []
    for match in DIRECTIVE_RE.finditer(line):
        label = match.group("label").lower()
        if label in policy.labels:
            actions.append(match.group("action").lower())
    return actions


def line_has_allow_next_line(
    line_number: int,
    lines: list[str],
    policy: LintDirectivePolicy,
) -> bool:
    if line_number <= 1:
        return False
    index = line_number - 2
    while index >= 0:
        line = lines[index]
        actions = matching_directive_actions(line, policy)
        if "allow-next-line" in actions:
            return True
        if not is_comment_line(line):
            return False
        index -= 1
    return False


def line_is_inside_allow_block(
    line_number: int,
    lines: list[str],
    policy: LintDirectivePolicy,
) -> bool:
    allow_depth = 0
    for line in lines[: line_number - 1]:
        for action in matching_directive_actions(line, policy):
            if action == "allow-start":
                allow_depth += 1
            elif action == "allow-end":
                allow_depth = max(0, allow_depth - 1)
    return allow_depth > 0


def line_is_suppressed(
    line_number: int,
    lines: list[str],
    policy: LintDirectivePolicy,
) -> bool:
    return line_has_allow_next_line(line_number, lines, policy) or line_is_inside_allow_block(
        line_number,
        lines,
        policy,
    )


def classify_rust_test_scope(
    lines: list[str],
    *,
    treat_all_lines_as_test: bool = False,
) -> list[bool]:
    if treat_all_lines_as_test:
        return [True] * len(lines)

    scope: list[bool] = []
    cfg_test_pending = False
    cfg_test_attribute_active = False
    test_block_depth: int | None = None
    brace_balance = 0

    for line in lines:
        stripped = line.strip()

        if "#[cfg(test)]" in stripped:
            cfg_test_pending = True
            cfg_test_attribute_active = True
            scope.append(True)
            continue

        if cfg_test_pending and stripped.startswith("#[") and stripped.endswith("]"):
            scope.append(True)
            continue

        current_scope_is_test = test_block_depth is not None or cfg_test_pending
        scope.append(current_scope_is_test)

        open_braces = line.count("{")
        close_braces = line.count("}")

        if cfg_test_pending and open_braces > 0:
            brace_balance = open_braces - close_braces
            test_block_depth = max(brace_balance, 0)
            cfg_test_pending = False
            cfg_test_attribute_active = False
        elif cfg_test_pending and stripped.endswith(";"):
            cfg_test_pending = False
            cfg_test_attribute_active = False
        elif test_block_depth is not None:
            brace_balance += open_braces - close_braces
            test_block_depth = max(brace_balance, 0)
            if test_block_depth == 0:
                test_block_depth = None
        elif cfg_test_attribute_active and stripped and not stripped.startswith("#["):
            cfg_test_pending = False
            cfg_test_attribute_active = False

    return scope
