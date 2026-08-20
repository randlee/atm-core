#!/usr/bin/env python3
"""Persist and render public-safe AI.40 local-transport benchmark evidence."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_schema import (
    BenchmarkSchemaError,
    BenchmarkSummary,
    SUMMARY_SCHEMA_VERSION,
    compact_evidence,
)


REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_NAME = "send-message-benchmark"
REPORT_HTML = f"{REPORT_NAME}.html"
REPORT_DIR = REPORTS_ROOT / REPORT_NAME
ENVELOPE_SCHEMA_VERSION = 1
AI40_SCHEMA_VERSION = SUMMARY_SCHEMA_VERSION
SUPPORTED_TRANSPORTS = frozenset({"uds", "tcp"})
SUPPORTED_FRAMES = frozenset({1, 2, 4, 8, 16, 64})
SAFE_LABEL = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")


class BenchmarkReportError(ValueError):
    """The benchmark result cannot be published as public evidence."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def parse_utc(value: Any, source: Path) -> str:
    if not isinstance(value, str) or not value:
        raise BenchmarkReportError(f"{source}: generated_at must be a non-empty UTC timestamp")
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError as error:
        raise BenchmarkReportError(f"{source}: generated_at is not ISO-8601") from error
    if parsed.tzinfo is None or parsed.utcoffset() != timezone.utc.utcoffset(parsed):
        raise BenchmarkReportError(f"{source}: generated_at must include UTC timezone")
    return parsed.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def safe_label(value: Any, field: str, source: Path) -> str:
    if not isinstance(value, str) or not SAFE_LABEL.fullmatch(value):
        raise BenchmarkReportError(f"{source}: {field} is not a safe opaque label")
    return value


def safe_artifact_id(value: str) -> str:
    candidate = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-")[:128]
    if not candidate or not SAFE_ID.fullmatch(candidate):
        raise BenchmarkReportError(f"unsafe benchmark artifact id: {value!r}")
    return candidate


def migrate_result(payload: dict[str, Any], source: Path) -> dict[str, Any]:
    """Convert legacy interval traces into the current compact schema."""
    version = payload.get("schema_version")
    if version == AI40_SCHEMA_VERSION:
        return payload
    if version in {3, 4}:
        migrated = dict(payload)
        migrated["schema_version"] = AI40_SCHEMA_VERSION
        migrated["peer_wire_security"] = (
            "not_applicable" if migrated.get("transport") == "uds" else "plaintext_test"
        )
        migrated["platform"] = "unknown"
        migrated["hook_mode"] = "unknown"
        migrated["evidence_scope"] = (
            "synthetic_validation"
            if str(migrated.get("host_label", "")).startswith("ao4-")
            else "unknown"
        )
        migrated["migration"] = {"from_schema_version": version}
        return migrated
    if version not in {1, 2} or isinstance(version, bool):
        raise BenchmarkReportError(
            f"{source}: expected benchmark schema version 1, 2, 3, 4, or {AI40_SCHEMA_VERSION}"
        )
    samples = payload.get("samples", payload.get("runs", []))
    if not isinstance(samples, list):
        raise BenchmarkReportError(f"{source}: legacy samples must be a list")
    legacy = {
        **payload,
        "schema_version": 2,
        "run_duration_s": payload.get("run_duration_s", payload.get("duration_seconds", 0)),
        "runs": samples if samples and isinstance(samples[0], dict) and "intervals" in samples[0] else [{"intervals": samples}],
        "minimum_sample_count": payload.get("minimum_sample_count", len(samples)),
        "sample_count": payload.get("sample_count", len(samples)),
        "target_duration_s": payload.get("target_duration_s", payload.get("run_duration_s", payload.get("duration_seconds", 0))),
        "platform": payload.get("platform", "unknown"),
        "hook_mode": payload.get("hook_mode", "unknown"),
        "evidence_scope": payload.get(
            "evidence_scope",
            "synthetic_validation" if str(payload.get("host_label", "")).startswith("ao4-") else "unknown",
        ),
    }
    try:
        result = compact_evidence(legacy).model_dump(mode="json")
    except BenchmarkSchemaError as error:
        raise BenchmarkReportError(f"{source}: cannot compact legacy interval evidence: {error}") from error
    result["migration"] = {"from_schema_version": version}
    return result


def validate_result(payload: dict[str, Any], source: Path) -> dict[str, Any]:
    payload = migrate_result(payload, source)
    migration = payload.pop("migration", None)
    try:
        result = BenchmarkSummary.model_validate(payload).model_dump(mode="json")
    except Exception as error:
        raise BenchmarkReportError(f"{source}: invalid benchmark summary: {error}") from error
    if migration is not None:
        result["migration"] = migration
    return result


def load_result(source: Path) -> dict[str, Any]:
    try:
        payload = json.loads(source.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise BenchmarkReportError(f"{source}: invalid JSON") from error
    if not isinstance(payload, dict):
        raise BenchmarkReportError(f"{source}: result must be a JSON object")
    return validate_result(payload, source)


def immutable_write(path: Path, content: str) -> bool:
    """Write once; repeated identical writes are idempotent, mutations fail."""
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        if path.read_text(encoding="utf-8") != content:
            raise BenchmarkReportError(f"immutable artifact already exists with different content: {path}")
        return False
    path.write_text(content, encoding="utf-8")
    return True


def result_id(result: dict[str, Any], _source: Path | None = None) -> str:
    stamp = result["generated_at"].replace("-", "").replace(":", "").replace("T", "-").replace("Z", "")
    migration_suffix = (
        f"-migrated-v{AI40_SCHEMA_VERSION}"
        if result.get("migration") is not None
        else ""
    )
    return safe_artifact_id(
        f"{stamp}-{result['host_label']}-{result['transport']}-"
        f"{result['peer_wire_security']}-f{result['frames_per_connection']}{migration_suffix}"
    )


def compose(template: Path, variables: dict[str, Any], output: Path) -> None:
    variables_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
            json.dump(variables, handle, sort_keys=True)
            variables_path = Path(handle.name)
        completed = subprocess.run(
            ["sc-compose", "render", "--root", str(ROOT), "--file", str(template), "--var-file", str(variables_path), "--output", str(output)],
            cwd=ROOT, capture_output=True, text=True, check=False,
        )
        if completed.returncode != 0:
            raise BenchmarkReportError(f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}")
    finally:
        if variables_path is not None:
            variables_path.unlink(missing_ok=True)


def evidence_sources(report_dir: Path | None = None) -> list[Path]:
    """Return public benchmark summaries and legacy sources, never envelopes."""
    report_dir = REPORT_DIR if report_dir is None else report_dir
    if not report_dir.is_dir():
        return []
    return [
        path for path in sorted(report_dir.glob("*.json"))
        if not path.name.endswith(".envelope.json")
    ]


def evidence_records(report_dir: Path | None = None) -> list[dict[str, Any]]:
    """Load one result per immutable ID, preferring a persisted current-schema copy."""
    report_dir = REPORT_DIR if report_dir is None else report_dir
    records: list[dict[str, Any]] = []
    for path in evidence_sources(report_dir):
        try:
            result = load_result(path)
        except BenchmarkReportError:
            continue
        canonical_path = report_dir / f"{result_id(result)}.json"
        if canonical_path.is_file() and path != canonical_path:
            continue
        records.append(result)
    return sorted(records, key=lambda item: (
        item["generated_at"], item["host_label"], item["transport"],
        item["peer_wire_security"], item["hook_mode"], item["frames_per_connection"],
    ))


def render_run(result: dict[str, Any], artifact_id: str, report_dir: Path | None = None) -> Path:
    report_dir = REPORT_DIR if report_dir is None else report_dir
    output = report_dir / f"{artifact_id}.xhtml"
    template = ROOT / "templates" / "benchmark-report" / "benchmark-run.xhtml.j2"
    metrics = result.get("metrics")
    sample_html = "<p>No interval completed.</p>"
    if metrics is not None:
        rates = metrics["admissions_per_second"]
        sample_html = (
            "<table><thead><tr><th>Intervals</th><th>Accepted</th><th>Responses</th>"
            "<th>Admissions/s (min / p50 / p95 / p99 / max)</th><th>Status</th></tr></thead><tbody>"
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.2f} / {:.2f} / {:.2f} / {} / {:.2f}</td><td>{}</td></tr>"
            "</tbody></table>"
        ).format(
            metrics["interval_count"], metrics["accepted_count"], metrics["response_count"],
            rates["min"], rates["p50"], rates["p95"],
            "n/a" if rates.get("p99") is None else f'{rates["p99"]:.2f}', rates["max"],
            "PASS" if result["passed"] else "FAIL",
        )
    direct_sqlite = result.get("direct_sqlite_message_write")
    direct_sqlite_html = "<p>Not captured by this historical run.</p>"
    if direct_sqlite is not None:
        direct_sqlite_html = (
            "<table><thead><tr><th>Requested</th><th>Accepted</th><th>Workers</th>"
            "<th>Elapsed seconds</th><th>Messages/second</th></tr></thead><tbody>"
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{:.3f}</td><td>{:.2f}</td></tr>"
            "</tbody></table>"
        ).format(
            direct_sqlite["requested_count"], direct_sqlite["accepted_count"],
            direct_sqlite["worker_count"], direct_sqlite["elapsed_seconds"],
            direct_sqlite["admissions_per_second"],
        )
    compose(
        template,
        {
            "title": f"ATM benchmark run — {artifact_id}",
            "artifact_id": artifact_id,
            "generated_at": result["generated_at"],
            "host_label": result["host_label"],
            "platform": result["platform"],
            "transport": result["transport"],
            "peer_wire_security": result["peer_wire_security"],
            "hook_mode": result["hook_mode"],
            "evidence_scope": result["evidence_scope"],
            "availability": result["availability"],
            "blocked_reason": result.get("blocked_reason", ""),
            "frames_per_connection": result["frames_per_connection"],
            "run_duration_s": result["run_duration_s"],
            "passed": result["passed"],
            "failure": result.get("failure", ""),
            "cleanup_failure": result.get("cleanup_failure", ""),
            "sample_html": sample_html,
            "direct_sqlite_html": direct_sqlite_html,
        },
        output,
    )
    return output


def latest_profile_results(records: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """Return the newest evidence for each host/transport/frame profile."""
    latest: dict[tuple[str, str, str, str, int], dict[str, Any]] = {}
    for result in records:
        key = (
            result["host_label"], result["transport"], result["peer_wire_security"], result["hook_mode"],
            result["frames_per_connection"],
        )
        previous = latest.get(key)
        if previous is None or result["generated_at"] > previous["generated_at"]:
            latest[key] = result
    return list(latest.values())


def current_campaign_results(records: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """Return the newest host/transport/revision evidence campaign.

    A benchmark campaign is one candidate build exercised on one host and one
    transport.  Keeping the source revision in the key prevents an older
    failing profile from changing the status of a newer candidate, while the
    aggregate table continues to retain every immutable result.
    """
    records = list(records)
    if not records:
        return []
    newest = max(records, key=lambda result: result["generated_at"])
    revision = newest.get("source_revision")
    if revision is None:
        # Legacy records did not carry a candidate revision.  For them, retain
        # the pre-existing newest-profile interpretation rather than allowing
        # a stale failure to poison a later recovery run.
        return [
            result
            for result in latest_profile_results(records)
            if (result["host_label"], result["transport"], result["peer_wire_security"], result["hook_mode"])
            == (newest["host_label"], newest["transport"], newest["peer_wire_security"], newest["hook_mode"])
        ]
    key = (newest["host_label"], newest["transport"], newest["peer_wire_security"], newest["hook_mode"], revision)
    return [
        result
        for result in records
        if (
            result["host_label"], result["transport"], result["peer_wire_security"],
            result["hook_mode"],
            result.get("source_revision"),
        ) == key
    ]


def campaign_status(results: Iterable[dict[str, Any]]) -> str:
    """Report PASS only for a complete, passing six-profile candidate."""
    results = list(results)
    if not results:
        return "INFO"
    if any(not result["passed"] for result in results):
        return "FAIL"
    frames = {result["frames_per_connection"] for result in results}
    return "PASS" if frames == SUPPORTED_FRAMES else "INFO"


def render_aggregate(records: Iterable[dict[str, Any]], report_root: Path | None = None) -> Path:
    report_root = REPORTS_ROOT if report_root is None else report_root
    records = list(records)
    rows = []
    for result in records:
        artifact_id = result_id(result)
        rows.append({
            "artifact_id": artifact_id,
            "generated_at": result["generated_at"],
            "host_label": result["host_label"],
            "transport": result["transport"],
            "peer_wire_security": result["peer_wire_security"],
            "hook_mode": result["hook_mode"],
            "evidence_scope": result["evidence_scope"],
            "frames_per_connection": result["frames_per_connection"],
            "passed": result["passed"],
            "direct_sqlite_admissions_per_second": (
                result["direct_sqlite_message_write"]["admissions_per_second"]
                if result.get("direct_sqlite_message_write") is not None
                else None
            ),
            "json_href": f"{REPORT_NAME}/{artifact_id}.json",
            "xhtml_href": f"{REPORT_NAME}/{artifact_id}.xhtml",
        })
    output = report_root / REPORT_HTML
    template = ROOT / "templates" / "benchmark-report" / "benchmark-report.html.j2"
    campaign = current_campaign_results(records)
    campaign_frames = {result["frames_per_connection"] for result in campaign}
    campaign_missing_frames = sorted(SUPPORTED_FRAMES - campaign_frames)
    campaign_reference = campaign[-1] if campaign else None
    compose(template, {
        "title": "ATM local transport benchmark", "generated_at": utc_now(),
        "status": campaign_status(campaign),
        "rows": rows,
        "campaign_host_label": campaign_reference["host_label"] if campaign_reference else "none",
        "campaign_transport": campaign_reference["transport"] if campaign_reference else "none",
        "campaign_peer_wire_security": campaign_reference["peer_wire_security"] if campaign_reference else "none",
        "campaign_hook_mode": campaign_reference["hook_mode"] if campaign_reference else "none",
        "campaign_source_revision": campaign_reference.get("source_revision") if campaign_reference else None,
        "campaign_profile_count": len(campaign),
        "campaign_passed_count": sum(result["passed"] for result in campaign),
        "campaign_failed_count": sum(not result["passed"] for result in campaign),
        "campaign_missing_frames": ", ".join(str(frame) for frame in campaign_missing_frames) or "none",
        "history_count": len(rows),
    }, output)
    return output


def regenerate_index() -> None:
    completed = subprocess.run(["just", "reports-index"], cwd=ROOT, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise BenchmarkReportError(f"reports-index failed: {completed.stderr.strip() or completed.stdout.strip()}")


def envelope_for(result: dict[str, Any]) -> str:
    """Return the AI.46 discovery envelope for one immutable benchmark run."""
    return json.dumps({
        "schema_version": ENVELOPE_SCHEMA_VERSION, "report_type": "benchmark",
        "generated_at": result["generated_at"], "host_label": result["host_label"],
        "report_html": REPORT_HTML,
    }, indent=2, sort_keys=True) + "\n"


def persist(source: Path) -> tuple[dict[str, Any], str]:
    result = load_result(source)
    artifact_id = result_id(result, source)
    artifact = json.dumps(result, indent=2, sort_keys=True) + "\n"
    immutable_write(REPORT_DIR / f"{artifact_id}.json", artifact)
    immutable_write(REPORT_DIR / f"{artifact_id}.envelope.json", envelope_for(result))
    return result, artifact_id


def process(inputs: list[Path]) -> int:
    errors: list[str] = []
    if not inputs:
        try:
            # A rebuild is the one-way schema-migration boundary. Legacy
            # summaries stay immutable historical inputs; their validated
            # current-schema copies are the report's canonical artifacts.
            for source in evidence_sources():
                persist(source)
            records = evidence_records()
            for result in records:
                artifact_id = result_id(result)
                immutable_write(REPORT_DIR / f"{artifact_id}.envelope.json", envelope_for(result))
                render_run(result, artifact_id)
            render_aggregate(records)
            regenerate_index()
        except (BenchmarkReportError, OSError) as error:
            errors.append(str(error))
    for source in inputs:
        try:
            result, artifact_id = persist(source)
            render_run(result, artifact_id)
            render_aggregate(evidence_records())
        except (BenchmarkReportError, OSError) as error:
            errors.append(str(error))
        finally:
            try:
                regenerate_index()
            except BenchmarkReportError as error:
                errors.append(str(error))
    for error in errors:
        print(f"benchmark-report: {error}", file=sys.stderr)
    return 1 if errors else 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, action="append", default=[], help="AI.40 result JSON (repeatable)")
    parser.add_argument("--rebuild", action="store_true", help="re-render existing evidence without adding input")
    args = parser.parse_args(argv)
    if not args.input and not args.rebuild:
        parser.error("provide --input PATH or --rebuild")
    return process(args.input)


if __name__ == "__main__":
    raise SystemExit(main())
