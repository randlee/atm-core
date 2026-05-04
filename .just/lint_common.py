#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
import re
import time


LOG_DIR = Path(".just/logs")
TIMESTAMP_FORMAT = "%Y%m%d%H%M%S"
LINT_NAME_RE = re.compile(r"[^A-Za-z0-9._-]+")


@dataclass(frozen=True)
class LintReport:
    lint_name: str
    passed: bool
    summary: str
    findings: list[str]
    transcript: list[str]
    duration_seconds: float
    log_path: Path


def discover_repo_root(explicit_root: str | None = None) -> Path:
    if explicit_root is not None:
        return Path(explicit_root).resolve()
    return Path(__file__).resolve().parent.parent


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
        return str(log_path.relative_to(repo_root))
    except ValueError:
        return str(log_path)


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
