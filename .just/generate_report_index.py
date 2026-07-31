#!/usr/bin/env python3
"""Generate and validate the durable public verification-report index.

Report producers may place one envelope at ``site/reports/<name>.json`` or
store run envelopes inside the same-named evidence directory.  Every envelope
points at a root-level ``<name>.html`` and its same-named evidence directory.
Ordinary evidence JSON without envelope fields is not a discovery input.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from datetime import datetime, timezone
import html
import json
from pathlib import Path, PurePosixPath
import re
import sys
from typing import Any, Iterable


SCHEMA_VERSION = 1
REPORT_TYPES = ("benchmark", "fuzz")
REPORTS_RELATIVE = Path("site/reports")
INDEX_NAME = "index.html"
HOST_LABEL_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
REPORT_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
REQUIRED_FIELDS = frozenset(
    {"schema_version", "report_type", "generated_at", "host_label", "report_html"}
)


class ReportIndexError(ValueError):
    """A report input cannot be published as public verification evidence."""


@dataclass(frozen=True)
class Envelope:
    schema_version: int
    report_type: str
    generated_at: datetime
    generated_at_text: str
    host_label: str
    report_html: str
    source: Path


@dataclass(frozen=True)
class ReportEntry:
    report_type: str
    report_html: str
    generated_at: datetime
    generated_at_text: str
    host_labels: tuple[str, ...]
    run_count: int


def _ensure_inside(path: Path, root: Path, description: str) -> None:
    try:
        resolved = path.resolve()
        resolved.relative_to(root.resolve())
    except ValueError as exc:
        raise ReportIndexError(f"{description} escapes the reports root: {path}") from exc


def _utc_timestamp(value: Any, source: Path) -> tuple[datetime, str]:
    if not isinstance(value, str) or not value:
        raise ReportIndexError(f"{source}: generated_at must be a non-empty string")
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        timestamp = datetime.fromisoformat(normalized)
    except ValueError as exc:
        raise ReportIndexError(f"{source}: generated_at is not ISO-8601: {value!r}") from exc
    if timestamp.tzinfo is None or timestamp.utcoffset() != timezone.utc.utcoffset(timestamp):
        raise ReportIndexError(f"{source}: generated_at must include UTC timezone")
    timestamp = timestamp.astimezone(timezone.utc)
    return timestamp, timestamp.isoformat().replace("+00:00", "Z")


def _safe_relative_html(value: Any, source: Path) -> str:
    if not isinstance(value, str) or not value:
        raise ReportIndexError(f"{source}: report_html must be a non-empty relative path")
    if "\\" in value or value.startswith("/") or "\x00" in value:
        raise ReportIndexError(f"{source}: report_html is not a safe relative path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise ReportIndexError(f"{source}: report_html is not a safe relative path: {value!r}")
    if len(path.parts) != 1 or path.suffix.lower() != ".html":
        raise ReportIndexError(f"{source}: report_html must be a root-level .html file")
    name = path.name[:-5]
    if not REPORT_NAME_RE.fullmatch(name):
        raise ReportIndexError(f"{source}: report_html has an unsafe report name: {value!r}")
    return path.as_posix()


def _safe_host_label(value: Any, source: Path) -> str:
    if not isinstance(value, str) or not HOST_LABEL_RE.fullmatch(value):
        raise ReportIndexError(
            f"{source}: host_label must be a 1-64 character opaque safe label"
        )
    return value


def parse_envelope(source: Path, reports_root: Path) -> Envelope:
    _ensure_inside(source, reports_root, "envelope")
    try:
        payload = json.loads(source.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ReportIndexError(f"{source}: malformed JSON envelope") from exc
    if not isinstance(payload, dict):
        raise ReportIndexError(f"{source}: envelope must be a JSON object")
    missing = REQUIRED_FIELDS - payload.keys()
    if missing:
        raise ReportIndexError(f"{source}: missing envelope fields: {', '.join(sorted(missing))}")
    unexpected_fields = set(payload) - REQUIRED_FIELDS
    if unexpected_fields:
        raise ReportIndexError(
            f"{source}: unsupported public fields: {', '.join(sorted(unexpected_fields))}"
        )
    schema_version = payload["schema_version"]
    if schema_version != SCHEMA_VERSION or isinstance(schema_version, bool):
        raise ReportIndexError(
            f"{source}: schema_version must be integer {SCHEMA_VERSION}"
        )
    report_type = payload["report_type"]
    if report_type not in REPORT_TYPES:
        raise ReportIndexError(
            f"{source}: report_type must be one of {', '.join(REPORT_TYPES)}"
        )
    generated_at, generated_at_text = _utc_timestamp(payload["generated_at"], source)
    host_label = _safe_host_label(payload["host_label"], source)
    report_html = _safe_relative_html(payload["report_html"], source)
    html_path = reports_root / report_html
    evidence_dir = reports_root / Path(report_html).stem
    _ensure_inside(html_path, reports_root, "report HTML")
    _ensure_inside(evidence_dir, reports_root, "evidence directory")
    if not html_path.is_file():
        raise ReportIndexError(f"{source}: missing report HTML: {report_html}")
    if not evidence_dir.is_dir():
        raise ReportIndexError(
            f"{source}: missing same-named evidence directory: {evidence_dir.name}/"
        )
    return Envelope(
        schema_version=schema_version,
        report_type=report_type,
        generated_at=generated_at,
        generated_at_text=generated_at_text,
        host_label=host_label,
        report_html=report_html,
        source=source,
    )


def discover_envelopes(reports_root: Path) -> list[Envelope]:
    if not reports_root.exists():
        return []
    if not reports_root.is_dir():
        raise ReportIndexError(f"reports root is not a directory: {reports_root}")
    envelopes: list[Envelope] = []
    for source in sorted(reports_root.rglob("*.json")):
        if source.name == "index.json":
            continue
        is_root_envelope = source.parent == reports_root
        is_explicit_envelope = source.name.endswith(".envelope.json")
        try:
            payload = json.loads(source.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError):
            if is_root_envelope or is_explicit_envelope:
                envelopes.append(parse_envelope(source, reports_root))
            continue
        if not isinstance(payload, dict):
            if is_root_envelope or is_explicit_envelope:
                envelopes.append(parse_envelope(source, reports_root))
            continue
        if is_root_envelope or is_explicit_envelope or {"report_type", "report_html"} & payload.keys():
            envelopes.append(parse_envelope(source, reports_root))
    return envelopes


def aggregate_entries(envelopes: Iterable[Envelope]) -> list[ReportEntry]:
    grouped: dict[tuple[str, str], list[Envelope]] = {}
    for envelope in envelopes:
        grouped.setdefault((envelope.report_type, envelope.report_html), []).append(envelope)
    entries: list[ReportEntry] = []
    for (report_type, report_html), group in grouped.items():
        newest = max(group, key=lambda item: (item.generated_at, item.source.name))
        entries.append(
            ReportEntry(
                report_type=report_type,
                report_html=report_html,
                generated_at=newest.generated_at,
                generated_at_text=newest.generated_at_text,
                host_labels=tuple(sorted({item.host_label for item in group})),
                run_count=len(group),
            )
        )
    return sorted(
        entries,
        key=lambda item: (REPORT_TYPES.index(item.report_type), -item.generated_at.timestamp(), item.report_html),
    )


def _entry_html(entry: ReportEntry) -> str:
    report_name = Path(entry.report_html).stem
    details = [html.escape(entry.report_type)]
    if entry.run_count != 1:
        details.append(f"{entry.run_count} runs")
    details.append("hosts: " + ", ".join(html.escape(label) for label in entry.host_labels))
    return (
        "      <li>"
        f'<a href="{html.escape(entry.report_html, quote=True)}">{html.escape(report_name)}</a>'
        f'<time datetime="{html.escape(entry.generated_at_text, quote=True)}">'
        f"{html.escape(entry.generated_at_text)}</time>"
        f'<span class="report-meta">{" · ".join(details)}</span>'
        "</li>"
    )


def render_index(entries: Iterable[ReportEntry]) -> str:
    entries_by_type = {report_type: [] for report_type in REPORT_TYPES}
    for entry in entries:
        entries_by_type[entry.report_type].append(entry)
    sections: list[str] = []
    for report_type in REPORT_TYPES:
        title = report_type.capitalize()
        report_entries = entries_by_type[report_type]
        if report_entries:
            rows = "\n".join(_entry_html(entry) for entry in report_entries)
            body = f"\n    <ul>\n{rows}\n    </ul>"
        else:
            body = "\n    <p class=\"empty\">No reports available.</p>"
        sections.append(f"  <section><h2>{title}</h2>{body}\n  </section>")
    return (
        "<!doctype html>\n"
        '<html lang="en">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        '  <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        "  <title>ATM verification reports</title>\n"
        "  <style>body{font:16px system-ui,sans-serif;max-width:64rem;margin:2rem auto;padding:0 1rem;line-height:1.5}"
        "h1{margin-bottom:.25rem}ul{padding:0;list-style:none}li{display:grid;grid-template-columns:minmax(12rem,2fr) auto 1fr;gap:1rem;padding:.55rem 0;border-bottom:1px solid #ddd}"
        "time,.report-meta{color:#555;font-size:.9em}.empty{color:#666;font-style:italic}</style>\n"
        "</head>\n"
        "<body>\n"
        "  <h1>ATM verification reports</h1>\n"
        "  <p>Generated from schema-validated public report envelopes.</p>\n"
        + "\n".join(sections)
        + "\n</body>\n</html>\n"
    )


def build_index(reports_root: Path) -> str:
    return render_index(aggregate_entries(discover_envelopes(reports_root)))


def write_or_check(repo_root: Path, check: bool) -> int:
    reports_root = repo_root / REPORTS_RELATIVE
    expected = build_index(reports_root)
    index_path = reports_root / INDEX_NAME
    if check:
        try:
            actual = index_path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as exc:
            raise ReportIndexError(f"missing or unreadable generated index: {index_path}") from exc
        if actual != expected:
            raise ReportIndexError(f"stale generated index: {index_path}")
        return 0
    reports_root.mkdir(parents=True, exist_ok=True)
    index_path.write_text(expected, encoding="utf-8")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate or check the durable report index.")
    parser.add_argument("--check", action="store_true", help="fail if the generated index is stale")
    parser.add_argument("--root", type=Path, help="repository root (defaults to the parent of .just)")
    args = parser.parse_args(argv[1:])
    repo_root = args.root.resolve() if args.root else Path(__file__).resolve().parents[1]
    try:
        return write_or_check(repo_root, args.check)
    except ReportIndexError as exc:
        print(f"report-index: error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
