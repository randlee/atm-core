#!/usr/bin/env python3
"""Generate and validate the durable public verification-report index.

Report producers may place one envelope at ``site/reports/<name>.json`` or
store a run envelope beside a nested report index. Every envelope points at a
safe HTML path relative to ``site/reports``. Ordinary evidence JSON without
envelope fields is not a discovery input.
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

REPOSITORY_ROOT = Path(__file__).resolve().parents[1]
if str(REPOSITORY_ROOT) not in sys.path:
    sys.path.insert(0, str(REPOSITORY_ROOT))

from scripts.report_runtime import ReportRuntimeError, resolve_procedure_page, resolve_procedure_revision


SCHEMA_VERSION = 1
REPORT_TYPES = ("benchmark", "fuzz", "smoke")
REPORTS_RELATIVE = Path("site/reports")
INDEX_NAME = "index.html"
HISTORY_DIRECTORY = "history"
HOST_LABEL_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
REPORT_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
REQUIRED_FIELDS = frozenset(
    {"schema_version", "report_type", "generated_at", "host_label", "report_html"}
)
# New benchmark producers may include in-band execution provenance.  Historical
# envelopes do not carry it, so discovery must accept both shapes.
OPTIONAL_FIELDS = frozenset({
    "execution_identity", "measurement_note", "effective_lane_settings", "ratchet",
    "source_revision", "procedure",
})
SMOKE_STATUS_VALUES = frozenset({"PASS", "FAIL"})


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
    status: str | None = None
    source_revision: str | None = None
    procedure: str | None = None
    procedure_html: str | None = None
    procedure_inferred: bool = False


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
    if path.suffix.lower() != ".html":
        raise ReportIndexError(f"{source}: report_html must be an .html file")
    name = path.stem
    if not REPORT_NAME_RE.fullmatch(name):
        raise ReportIndexError(f"{source}: report_html has an unsafe report name: {value!r}")
    return path.as_posix()


def _safe_host_label(value: Any, source: Path) -> str:
    if not isinstance(value, str) or not HOST_LABEL_RE.fullmatch(value):
        raise ReportIndexError(
            f"{source}: host_label must be a 1-64 character opaque safe label"
        )
    return value


def _smoke_status(value: Any, source: Path) -> str:
    if value not in SMOKE_STATUS_VALUES:
        raise ReportIndexError(
            f"{source}: smoke status must be one of {', '.join(sorted(SMOKE_STATUS_VALUES))}"
        )
    return value


def _smoke_run_timestamp(value: Any, source: Path) -> tuple[datetime, str]:
    if not isinstance(value, str):
        raise ReportIndexError(f"{source}: smoke run_id must be a string")
    match = re.fullmatch(r"(\d{8}T\d{6})(\d{0,6})Z", value)
    if match is None:
        raise ReportIndexError(f"{source}: smoke run_id is not a UTC timestamp")
    try:
        timestamp = datetime.strptime(match.group(1), "%Y%m%dT%H%M%S").replace(
            tzinfo=timezone.utc,
            microsecond=int(match.group(2).ljust(6, "0") or "0"),
        )
    except ValueError as exc:
        raise ReportIndexError(f"{source}: smoke run_id is not a valid timestamp") from exc
    return timestamp, timestamp.isoformat().replace("+00:00", "Z")


def _source_revision(value: Any, source: Path) -> str | None:
    if value is None:
        return None
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ReportIndexError(f"{source}: source_revision must be a lowercase Git object ID")
    return value


def _sibling_feature(source: Path) -> str | None:
    for candidate in sorted(source.parent.glob("*.json")):
        if candidate == source:
            continue
        try: payload = json.loads(candidate.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError): continue
        if isinstance(payload, dict) and isinstance(payload.get("feature"), str):
            return payload["feature"]
    return None


def _procedure_for_feature(feature: str) -> str:
    if feature in {"graft-hermes", "colima-hermes-skills"}:
        return feature
    return "smoke-" + ("local-ip" if feature == "local-up" else feature)


def _manifest(reports_root: Path) -> dict[str, list[dict[str, Any]]]:
    path = reports_root / "procedures" / "manifest.json"
    if not path.is_file():
        raise ReportIndexError(f"missing procedure manifest: {path}")
    try: payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise ReportIndexError(f"malformed procedure manifest: {path}") from exc
    procedures = payload.get("procedures") if isinstance(payload, dict) else None
    if not isinstance(procedures, list):
        raise ReportIndexError(f"malformed procedure manifest: {path}")
    return {item["procedure"]: item["revisions"] for item in procedures if isinstance(item, dict) and isinstance(item.get("procedure"), str) and isinstance(item.get("revisions"), list)}


def _procedure_for(envelope: Envelope) -> str:
    if envelope.procedure:
        return envelope.procedure
    if envelope.report_type == "benchmark":
        return envelope.source.name.removesuffix(".json")
    if envelope.report_type == "smoke":
        feature = _sibling_feature(envelope.source)
        if feature == "graft-hermes" or feature == "colima-hermes-skills": return feature
        return "smoke-" + ("local-ip" if feature == "local-up" else feature or "unknown")
    target = None
    for candidate in sorted(envelope.source.parent.glob("*.json")):
        data = None
        try: data = json.loads(candidate.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, json.JSONDecodeError): pass
        campaign = data.get("campaign", data) if isinstance(data, dict) else {}
        if isinstance(campaign, dict) and isinstance(campaign.get("target"), str): target = campaign["target"]; break
    if target is None and envelope.source.parent.name == "fuzz": target = "unknown"
    return "fuzz-" + (target or "unknown")


def resolve_procedures(envelopes: list[Envelope], reports_root: Path) -> list[Envelope]:
    if not envelopes:
        return []
    manifest = _manifest(reports_root)
    resolved: list[Envelope] = []
    for envelope in envelopes:
        procedure = _procedure_for(envelope)
        revisions = manifest.get(procedure)
        if revisions is None:
            raise ReportIndexError(f"{envelope.source}: procedure {procedure} has no page")
        source_revision = envelope.source_revision
        inferred = source_revision is None
        selected = None
        if source_revision:
            try:
                selected = resolve_procedure_revision(
                    revisions,
                    source_revision,
                    root=reports_root.parent.parent,
                    generated_at=envelope.generated_at_text,
                )
            except ReportRuntimeError as error:
                raise ReportIndexError(f"{envelope.source}: {error}") from error
        if selected is None and inferred:
            dated = [item for item in revisions if isinstance(item.get("date"), str) and item["date"] <= envelope.generated_at_text[:10]]
            selected = max(dated, key=lambda item: item["date"], default=None)
        if selected is None:
            revision = source_revision or "none"
            raise ReportIndexError(f"{envelope.source}: procedure {procedure} has no page for revision {revision} (run date {envelope.generated_at_text})")
        try:
            page = resolve_procedure_page(
                procedure,
                selected.get("rev"),
                root=reports_root.parent.parent,
                generated_at=envelope.generated_at_text,
                error_type=ReportIndexError,
            )
        except ReportIndexError as error:
            raise ReportIndexError(f"{envelope.source}: {error}") from error
        if page is None:
            raise ReportIndexError(
                f"{envelope.source}: procedure {procedure} has no page for revision {selected.get('rev', 'none')}"
            )
        resolved.append(Envelope(**{**envelope.__dict__, "procedure": procedure, "procedure_html": page.html, "procedure_inferred": inferred}))
    return resolved


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
    report_type = payload["report_type"]
    if report_type not in REPORT_TYPES:
        raise ReportIndexError(
            f"{source}: report_type must be one of {', '.join(REPORT_TYPES)}"
        )
    allowed_fields = REQUIRED_FIELDS | OPTIONAL_FIELDS | ({"status"} if report_type == "smoke" else set())
    unexpected_fields = set(payload) - allowed_fields
    if unexpected_fields:
        raise ReportIndexError(
            f"{source}: unsupported public fields: {', '.join(sorted(unexpected_fields))}"
        )
    schema_version = payload["schema_version"]
    if schema_version != SCHEMA_VERSION or isinstance(schema_version, bool):
        raise ReportIndexError(
            f"{source}: schema_version must be integer {SCHEMA_VERSION}"
        )
    generated_at, generated_at_text = _utc_timestamp(payload["generated_at"], source)
    procedure = payload.get("procedure")
    if procedure is not None and (not isinstance(procedure, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", procedure)):
        raise ReportIndexError(f"{source}: procedure must be a safe procedure id")
    host_label = _safe_host_label(payload["host_label"], source)
    report_html = _safe_relative_html(payload["report_html"], source)
    html_path = reports_root / report_html
    # A report producer may publish an index inside its evidence directory
    # (for example ``send-message-benchmark/index.html``).  Flat legacy
    # reports retain the original same-stem convention.
    relative_report = Path(report_html)
    evidence_dir = (
        reports_root / relative_report.parent
        if relative_report.parent != Path(".")
        else reports_root / relative_report.stem
    )
    _ensure_inside(html_path, reports_root, "report HTML")
    _ensure_inside(evidence_dir, reports_root, "evidence directory")
    if not html_path.is_file():
        raise ReportIndexError(f"{source}: missing report HTML: {report_html}")
    if report_type != "smoke" and not evidence_dir.is_dir():
        raise ReportIndexError(
            f"{source}: missing same-named evidence directory: {evidence_dir.name}/"
        )
    if report_type == "smoke" and source.parent != html_path.parent:
        raise ReportIndexError(
            f"{source}: smoke envelope must be stored beside its run index"
        )
    return Envelope(
        schema_version=schema_version,
        report_type=report_type,
        generated_at=generated_at,
        generated_at_text=generated_at_text,
        host_label=host_label,
        report_html=report_html,
        source=source,
        status=_smoke_status(payload["status"], source) if report_type == "smoke" else None,
        source_revision=_source_revision(payload.get("source_revision"), source),
        procedure=procedure,
    )


def parse_smoke_result(source: Path, reports_root: Path, payload: dict[str, Any]) -> Envelope:
    """Adapt a pre-envelope smoke result so historical runs remain browseable."""
    _ensure_inside(source, reports_root, "smoke result")
    required = {"feature", "platform", "host", "run_id", "status", "cases"}
    if not required.issubset(payload):
        missing = ", ".join(sorted(required - payload.keys()))
        raise ReportIndexError(f"{source}: smoke result is missing fields: {missing}")
    host_label = _safe_host_label(payload["host"], source)
    _safe_host_label(payload["platform"], source)
    timestamp, timestamp_text = _smoke_run_timestamp(payload["run_id"], source)
    html_path = source.parent / "index.html"
    _ensure_inside(html_path, reports_root, "smoke report HTML")
    if not html_path.is_file():
        raise ReportIndexError(f"{source}: missing smoke run index.html")
    return Envelope(
        schema_version=SCHEMA_VERSION,
        report_type="smoke",
        generated_at=timestamp,
        generated_at_text=timestamp_text,
        host_label=host_label,
        report_html=html_path.relative_to(reports_root).as_posix(),
        source=source,
        status=_smoke_status(payload["status"], source),
        source_revision=_source_revision(payload.get("source_revision"), source),
        procedure=_procedure_for_feature(payload["feature"]),
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
        # AO2.11 replaces benchmark's former many-per-run discovery sidecars
        # with one canonical directory index.  Preserve the sidecars as
        # historical evidence, but do not publish stale links from them.
        if (
            is_explicit_envelope
            and source.parent.name == "send-message-benchmark"
            and (reports_root / "send-message-benchmark.json").is_file()
        ):
            continue
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
        # A nested benchmark artifact may carry ``report_type`` as data while
        # intentionally lacking the public envelope's ``report_html`` field.
        # Treat only the complete pair as an envelope; using set intersection
        # here incorrectly rejected valid family artifacts during indexing.
        if is_root_envelope or is_explicit_envelope or {"report_type", "report_html"} <= payload.keys():
            envelopes.append(parse_envelope(source, reports_root))
        elif (
            source.parent.parent.parent.parent.name == "smoke"
            and not (source.parent / "smoke.envelope.json").is_file()
            and {"feature", "platform", "host", "run_id", "status", "cases"}.issubset(payload)
        ):
            envelopes.append(parse_smoke_result(source, reports_root, payload))
    return resolve_procedures(envelopes, reports_root)


@dataclass(frozen=True)
class Classification:
    """One major kind of test run (a procedure family): every run of every procedure in it."""

    family: str
    runs: tuple[Envelope, ...]  # newest first

    @property
    def title(self) -> str:
        return self.family.capitalize()

    @property
    def latest(self) -> Envelope:
        return self.runs[0]

    @property
    def history_html(self) -> str:
        return f"{HISTORY_DIRECTORY}/{self.family}.html"


def _procedure_families(reports_root: Path) -> dict[str, str]:
    path = reports_root / "procedures" / "manifest.json"
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError):
        return {}
    procedures = payload.get("procedures") if isinstance(payload, dict) else None
    if not isinstance(procedures, list):
        return {}
    return {
        item["procedure"]: item["family"]
        for item in procedures
        if isinstance(item, dict) and isinstance(item.get("procedure"), str) and isinstance(item.get("family"), str)
    }


def _lane(procedure: str | None, family: str) -> str:
    """The test within its family: ``smoke-local-ip`` -> ``local ip``; ``send-message-benchmark`` -> ``send message``."""
    words = (procedure or "unknown").split("-")
    if words and words[0] == family:
        words = words[1:]
    elif words and words[-1] == family:
        words = words[:-1]
    return " ".join(words) or family


def classify(envelopes: Iterable[Envelope], reports_root: Path) -> list[Classification]:
    families = _procedure_families(reports_root)
    grouped: dict[str, list[Envelope]] = {}
    for envelope in envelopes:
        family = families.get(envelope.procedure or "", envelope.report_type)
        grouped.setdefault(family, []).append(envelope)
    classifications = [
        Classification(family=family, runs=tuple(sorted(runs, key=lambda item: (item.generated_at, item.source.name), reverse=True)))
        for family, runs in grouped.items()
    ]
    return sorted(
        classifications,
        key=lambda item: (REPORT_TYPES.index(item.family) if item.family in REPORT_TYPES else len(REPORT_TYPES), item.family),
    )


def _when(envelope: Envelope) -> str:
    return envelope.generated_at.strftime("%Y-%m-%d %H:%M UTC")


def _time_html(envelope: Envelope) -> str:
    return (
        f'<time datetime="{html.escape(envelope.generated_at_text, quote=True)}">'
        f"{html.escape(_when(envelope))}</time>"
    )


def _result_html(envelope: Envelope) -> str:
    if envelope.status is None:
        return '<span class="result">report</span>'
    return f'<span class="result {html.escape(envelope.status.lower())}">{html.escape(envelope.status)}</span>'


def _procedure_html(envelope: Envelope, prefix: str) -> str:
    if not envelope.procedure_html:
        return ""
    revision = envelope.procedure_html.rsplit("/", 1)[-1][:8]
    inferred = ' <span class="inferred">(inferred from run date)</span>' if envelope.procedure_inferred else ""
    return (
        f'<a class="procedure" href="{html.escape(prefix + envelope.procedure_html, quote=True)}">'
        f"{html.escape(envelope.procedure or 'unknown')} @ {html.escape(revision)}</a>{inferred}"
    )


def _run_count(classification: Classification) -> str:
    count = len(classification.runs)
    return f"{count} run" if count == 1 else f"{count} runs"


STYLE = (
    "body{font:16px system-ui,sans-serif;max-width:72rem;margin:2rem auto;padding:0 1rem;line-height:1.5;color:#1b1b1b}"
    "h1{margin-bottom:.25rem}p.lead{color:#555;margin-top:0}"
    "table{border-collapse:collapse;width:100%;margin:1rem 0}"
    "th{text-align:left;font-weight:600;color:#555;font-size:.85em;border-bottom:2px solid #c9c9c9;padding:.4rem .6rem}"
    "td{padding:.55rem .6rem;border-bottom:1px solid #e3e3e3;vertical-align:top}"
    "td.family{color:#555;white-space:nowrap}"
    "a{color:#0b57d0}a:visited{color:#6a3fb5}"
    ".result{font-weight:600}.result.pass{color:#1a7f37}.result.fail{color:#c62828}"
    ".inferred,.meta{color:#777;font-size:.85em}.empty{color:#666;font-style:italic}"
    "nav{margin-bottom:1rem}"
)


def _page(title: str, body: str) -> str:
    return (
        "<!doctype html>\n"
        '<html lang="en">\n'
        "<head>\n"
        '  <meta charset="utf-8">\n'
        '  <meta name="viewport" content="width=device-width, initial-scale=1">\n'
        f"  <title>{html.escape(title)}</title>\n"
        f"  <style>{STYLE}</style>\n"
        "</head>\n"
        "<body>\n"
        f"{body}"
        "</body>\n</html>\n"
    )


def render_index(classifications: Iterable[Classification]) -> str:
    rows: list[str] = []
    for item in classifications:
        latest = item.latest
        rows.append(
            "    <tr>"
            f'<td class="family"><strong>{html.escape(item.title)}</strong></td>'
            f'<td><a href="{html.escape(latest.report_html, quote=True)}">{_time_html(latest)}</a>'
            f'<span class="meta"> · {html.escape(_lane(latest.procedure, item.family))} · {html.escape(latest.host_label)}</span></td>'
            f"<td>{_result_html(latest)}</td>"
            f"<td>{_procedure_html(latest, '')}</td>"
            f'<td><a href="{html.escape(item.history_html, quote=True)}">{_run_count(item)}</a></td>'
            "</tr>"
        )
    if rows:
        table = (
            "  <table>\n"
            "    <thead><tr><th>Test</th><th>Latest run</th><th>Result</th><th>Procedure executed</th><th>History</th></tr></thead>\n"
            "    <tbody>\n" + "\n".join(rows) + "\n    </tbody>\n"
            "  </table>\n"
        )
    else:
        table = '  <p class="empty">No reports available.</p>\n'
    body = (
        "  <h1>ATM verification reports</h1>\n"
        '  <p class="lead">One row per test family. Open the latest run, the procedure it executed, or the full run history (newest first). Generated from schema-validated public report envelopes.</p>\n'
        + table
    )
    return _page("ATM verification reports", body)


def render_history(classification: Classification) -> str:
    prefix = "../"
    rows = "\n".join(
        "    <tr>"
        f'<td><a href="{html.escape(prefix + run.report_html, quote=True)}">{_time_html(run)}</a></td>'
        f"<td>{html.escape(_lane(run.procedure, classification.family))}</td>"
        f"<td>{html.escape(run.host_label)}</td>"
        f"<td>{_result_html(run)}</td>"
        f"<td>{_procedure_html(run, prefix)}</td>"
        f'<td class="meta">{html.escape(run.report_html.removesuffix("/index.html"))}</td>'
        "</tr>"
        for run in classification.runs
    )
    body = (
        f'  <nav><a href="{prefix}index.html">← All reports</a></nav>\n'
        f"  <h1>{html.escape(classification.title)}</h1>\n"
        f'  <p class="lead">{html.escape(_run_count(classification))}, newest first.</p>\n'
        "  <table>\n"
        "    <thead><tr><th>Run</th><th>Test</th><th>Host</th><th>Result</th><th>Procedure executed</th><th>Report path</th></tr></thead>\n"
        f"    <tbody>\n{rows}\n    </tbody>\n"
        "  </table>\n"
    )
    return _page(f"{classification.title} — run history", body)


def build_pages(reports_root: Path) -> dict[str, str]:
    """Every generated page under ``site/reports``, keyed by its relative path."""
    classifications = classify(discover_envelopes(reports_root), reports_root)
    pages = {INDEX_NAME: render_index(classifications)}
    for item in classifications:
        pages[item.history_html] = render_history(item)
    return pages


def build_index(reports_root: Path) -> str:
    return build_pages(reports_root)[INDEX_NAME]


def write_or_check(repo_root: Path, check: bool) -> int:
    reports_root = repo_root / REPORTS_RELATIVE
    expected = build_pages(reports_root)
    history_root = reports_root / HISTORY_DIRECTORY
    stale = sorted(
        path.relative_to(reports_root).as_posix()
        for path in (history_root.glob("*.html") if history_root.is_dir() else ())
        if path.relative_to(reports_root).as_posix() not in expected
    )
    if check:
        for relative, content in expected.items():
            path = reports_root / relative
            try:
                actual = path.read_text(encoding="utf-8")
            except (OSError, UnicodeError) as exc:
                raise ReportIndexError(f"missing or unreadable generated page: {path}") from exc
            if actual != content:
                raise ReportIndexError(f"stale generated page: {path}")
        if stale:
            raise ReportIndexError(f"stale history pages without a report: {', '.join(stale)}")
        return 0
    reports_root.mkdir(parents=True, exist_ok=True)
    for relative, content in expected.items():
        path = reports_root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    for relative in stale:
        (reports_root / relative).unlink()
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Generate or check the durable report index and per-test run history pages.")
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
