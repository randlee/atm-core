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
REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_NAME = "send-message-benchmark"
REPORT_HTML = f"{REPORT_NAME}.html"
REPORT_DIR = REPORTS_ROOT / REPORT_NAME
ENVELOPE_SCHEMA_VERSION = 1
AI40_SCHEMA_VERSION = 2
SUPPORTED_TRANSPORTS = frozenset({"uds", "tcp"})
SUPPORTED_FRAMES = frozenset({1, 2, 8, 16, 64})
SAFE_LABEL = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
ABSOLUTE_PATH = re.compile(r"(?:/Users/[^\s,;]+|/private/tmp/[^\s,;]+|/tmp/[^\s,;]+|[A-Za-z]:\\[^\s,;]+)")
SENSITIVE_KEYS = frozenset({"atm_home", "daemon_pid", "doctor", "endpoint", "release", "peer_host", "current_dir", "home_dir", "path"})
RUN_KEYS = frozenset({
    "interval", "accepted_count", "requested_count", "response_count", "elapsed_seconds",
    "admissions_per_second", "connections_per_second", "request_frames_per_second",
    "application_wire_bytes", "application_wire_bytes_per_second", "bytes_per_second",
    "latency_ms", "first_failure", "passed",
})


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


def public_string(value: str) -> str:
    return ABSOLUTE_PATH.sub("<redacted-path>", value)[:2000]


def public_value(value: Any) -> Any:
    if isinstance(value, str):
        return public_string(value)
    if isinstance(value, (int, float, bool)) or value is None:
        return value
    if isinstance(value, list):
        return [public_value(item) for item in value]
    if isinstance(value, dict):
        return {str(key): public_value(item) for key, item in value.items() if str(key) not in SENSITIVE_KEYS}
    return public_string(str(value))


def migrate_result(payload: dict[str, Any], source: Path) -> dict[str, Any]:
    """Migrate the short-lived AI.40 v1 fixture shape into schema v2."""
    if payload.get("schema_version") == 1:
        samples = payload.get("samples", payload.get("runs", []))
        if not isinstance(samples, list):
            raise BenchmarkReportError(f"{source}: legacy samples must be a list")
        payload = {
            **payload,
            "schema_version": AI40_SCHEMA_VERSION,
            "run_duration_s": payload.get("run_duration_s", payload.get("duration_seconds", 0)),
            "runs": samples if samples and isinstance(samples[0], dict) and "intervals" in samples[0] else [{"label": "legacy", "intervals": samples}],
            "migration": {"from_schema_version": 1},
        }
    if payload.get("schema_version") != AI40_SCHEMA_VERSION or isinstance(payload.get("schema_version"), bool):
        raise BenchmarkReportError(f"{source}: expected AI.40 schema_version {AI40_SCHEMA_VERSION}")
    return payload


def validate_result(payload: dict[str, Any], source: Path) -> dict[str, Any]:
    payload = migrate_result(payload, source)
    generated_at = parse_utc(payload.get("generated_at", utc_now()), source)
    host_label = safe_label(payload.get("host_label"), "host_label", source)
    transport = payload.get("transport")
    if transport not in SUPPORTED_TRANSPORTS:
        raise BenchmarkReportError(f"{source}: transport must be uds or tcp")
    frames = payload.get("frames_per_connection")
    if isinstance(frames, bool) or frames not in SUPPORTED_FRAMES:
        raise BenchmarkReportError(f"{source}: frames_per_connection must be one of 1, 2, 8, 16, 64")
    duration = payload.get("run_duration_s")
    if isinstance(duration, bool) or not isinstance(duration, (int, float)) or duration <= 0:
        raise BenchmarkReportError(f"{source}: run_duration_s must be positive")
    runs = payload.get("runs")
    if not isinstance(runs, list) or not runs:
        raise BenchmarkReportError(f"{source}: runs must be a non-empty list")
    public_runs: list[dict[str, Any]] = []
    for index, run in enumerate(runs):
        if not isinstance(run, dict):
            raise BenchmarkReportError(f"{source}: run {index} must be an object")
        intervals = run.get("intervals")
        if intervals is not None and not isinstance(intervals, list):
            raise BenchmarkReportError(f"{source}: run {index} intervals must be a list")
        retained = {key: public_value(value) for key, value in run.items() if key in RUN_KEYS}
        if intervals is not None:
            retained["intervals"] = [public_value(item) for item in intervals]
        if "label" in run:
            retained["label"] = public_string(str(run["label"]))
        public_runs.append(retained)
    result: dict[str, Any] = {
        "schema_version": AI40_SCHEMA_VERSION,
        "generated_at": generated_at,
        "host_label": host_label,
        "transport": transport,
        "frames_per_connection": frames,
        "run_duration_s": duration,
        "messages_per_connection": payload.get("messages_per_connection", frames),
        "runs": public_runs,
        "passed": bool(payload.get("passed", False)),
    }
    if "migration" in payload:
        result["migration"] = public_value(payload["migration"])
    for key in ("failure", "cleanup_failure"):
        if payload.get(key):
            result[key] = public_string(str(payload[key]))
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
    return safe_artifact_id(f"{stamp}-{result['host_label']}-{result['transport']}-f{result['frames_per_connection']}")


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


def evidence_records(report_dir: Path = REPORT_DIR) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    if not report_dir.is_dir():
        return records
    for path in sorted(report_dir.glob("*.json")):
        if path.name.endswith(".envelope.json"):
            continue
        try:
            records.append(load_result(path))
        except BenchmarkReportError:
            continue
    return sorted(records, key=lambda item: (item["generated_at"], item["host_label"], item["transport"], item["frames_per_connection"]))


def render_run(result: dict[str, Any], artifact_id: str, report_dir: Path = REPORT_DIR) -> Path:
    output = report_dir / f"{artifact_id}.xhtml"
    template = ROOT / "templates" / "benchmark-report" / "benchmark-run.xhtml.j2"
    sample_html = []
    for run in result["runs"]:
        rows = []
        for interval in run.get("intervals", []):
            rows.append(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>".format(
                    escape(str(interval.get("interval", "—"))),
                    escape(str(interval.get("accepted_count", "—"))),
                    escape(str(interval.get("response_count", "—"))),
                    escape(str(interval.get("elapsed_seconds", "—"))),
                    escape(str(interval.get("admissions_per_second", "—"))),
                    "PASS" if interval.get("passed") else "FAIL",
                )
            )
        sample_html.append(
            "<article class=\"benchmark-sample\"><h3>{}</h3><table><thead><tr>"
            "<th>Interval</th><th>Accepted</th><th>Responses</th><th>Elapsed s</th>"
            "<th>Admissions/s</th><th>Status</th></tr></thead><tbody>{}</tbody></table></article>".format(
                escape(str(run.get("label", "sample"))), "".join(rows)
            )
        )
    compose(
        template,
        {
            "title": f"ATM benchmark run — {artifact_id}",
            "artifact_id": artifact_id,
            "generated_at": result["generated_at"],
            "host_label": result["host_label"],
            "transport": result["transport"],
            "frames_per_connection": result["frames_per_connection"],
            "run_duration_s": result["run_duration_s"],
            "passed": result["passed"],
            "failure": result.get("failure", ""),
            "cleanup_failure": result.get("cleanup_failure", ""),
            "sample_html": "".join(sample_html),
        },
        output,
    )
    return output


def render_aggregate(records: Iterable[dict[str, Any]], report_root: Path = REPORTS_ROOT) -> Path:
    rows = []
    for result in records:
        artifact_id = result_id(result)
        rows.append({
            "artifact_id": artifact_id,
            "generated_at": result["generated_at"],
            "host_label": result["host_label"],
            "transport": result["transport"],
            "frames_per_connection": result["frames_per_connection"],
            "passed": result["passed"],
            "json_href": f"{REPORT_NAME}/{artifact_id}.json",
            "xhtml_href": f"{REPORT_NAME}/{artifact_id}.xhtml",
        })
    output = report_root / REPORT_HTML
    template = ROOT / "templates" / "benchmark-report" / "benchmark-report.html.j2"
    compose(template, {
        "title": "ATM local transport benchmark", "generated_at": utc_now(),
        "status": "PASS" if rows and all(row["passed"] for row in rows) else "FAIL" if rows else "INFO",
        "rows": rows, "profile_count": len(rows),
        "passed_count": sum(row["passed"] for row in rows),
        "failed_count": sum(not row["passed"] for row in rows),
    }, output)
    return output


def regenerate_index() -> None:
    completed = subprocess.run(["just", "reports-index"], cwd=ROOT, capture_output=True, text=True, check=False)
    if completed.returncode != 0:
        raise BenchmarkReportError(f"reports-index failed: {completed.stderr.strip() or completed.stdout.strip()}")


def persist(source: Path) -> tuple[dict[str, Any], str]:
    result = load_result(source)
    artifact_id = result_id(result, source)
    artifact = json.dumps(result, indent=2, sort_keys=True) + "\n"
    envelope = json.dumps({
        "schema_version": ENVELOPE_SCHEMA_VERSION, "report_type": "benchmark",
        "generated_at": result["generated_at"], "host_label": result["host_label"],
        "report_html": REPORT_HTML,
    }, indent=2, sort_keys=True) + "\n"
    immutable_write(REPORT_DIR / f"{artifact_id}.json", artifact)
    immutable_write(REPORT_DIR / f"{artifact_id}.envelope.json", envelope)
    return result, artifact_id


def process(inputs: list[Path]) -> int:
    errors: list[str] = []
    if not inputs:
        try:
            render_aggregate(evidence_records())
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
