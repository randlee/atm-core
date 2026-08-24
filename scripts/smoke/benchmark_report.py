#!/usr/bin/env python3
"""Persist and render public-safe AI.40 local-transport benchmark evidence."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
from html import escape
import hashlib
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
    BaselineSet,
    BenchmarkRunResult,
    BenchmarkSchemaError,
    BenchmarkSummary,
    LEGACY_SUMMARY_SCHEMA_VERSION,
    artifact_id as derive_artifact_id,
    compact_evidence,
)


REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_NAME = "send-message-benchmark"
REPORT_HTML = f"{REPORT_NAME}.html"
REPORT_DIR = REPORTS_ROOT / REPORT_NAME
BASELINES_PATH = REPORT_DIR / "baselines.json"
HISTORICAL_IMPORTS_NAME = "historical-imports.json"
ENVELOPE_SCHEMA_VERSION = 1
# AO2.10 introduces v4 emission.  This reader continues to accept v3
# artifacts unchanged until AO2.12 performs the separately reviewed history
# migration; reports must never require rewriting existing evidence in place.
AI40_SCHEMA_VERSION = LEGACY_SUMMARY_SCHEMA_VERSION
SUPPORTED_TRANSPORTS = frozenset({"uds", "tcp"})
SUPPORTED_FRAMES = frozenset({1, 2, 4, 8, 16, 64})
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
GIT_REVISION = re.compile(r"^[0-9a-f]{40}$")
CAMPAIGN_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")

TARGET_ORDER = ("sqlite", "uds", "tcp", "tcp-tls")


class BenchmarkReportError(ValueError):
    """The benchmark result cannot be published as public evidence."""


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


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
    if version not in {1, 2} or isinstance(version, bool):
        raise BenchmarkReportError(f"{source}: expected benchmark schema version 1, 2, or {AI40_SCHEMA_VERSION}")
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
    }
    try:
        result = compact_evidence(legacy).model_dump(mode="json")
    except BenchmarkSchemaError as error:
        raise BenchmarkReportError(f"{source}: cannot compact legacy interval evidence: {error}") from error
    result["migration"] = {"from_schema_version": version}
    return result


def validate_result(payload: dict[str, Any], source: Path) -> dict[str, Any]:
    if payload.get("schema_version") == 4:
        try:
            result = BenchmarkRunResult.model_validate(payload).model_dump(mode="json")
        except Exception as error:
            raise BenchmarkReportError(f"{source}: invalid v4 benchmark result: {error}") from error
        target = result["target"]
        return {
            **result,
            # Rendering remains compatible while AO2.11 owns the presentation
            # refactor; these are descriptive projections, never acceptance
            # inputs.
            "transport": "sqlite" if target == "sqlite" else ("uds" if target == "uds" else "tcp"),
            "peer_wire_security": (
                None if target == "sqlite" else ("plaintext-test" if target == "tcp" else "mutual-tls")
            ),
            "benchmark_target": target,
            "passed": result["status"] == "PASS",
            "host_os": result["os"],
            "host_arch": "unknown",
            "daemon_version": "recorded by binary hash",
            "run_duration_s": 0.0,
        }
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
    if result.get("schema_version") == 4:
        return derive_artifact_id(
            campaign_id=str(result["campaign_id"]), target=str(result["target"]),
        )
    stamp = result["generated_at"].replace("-", "").replace(":", "").replace("T", "-").replace("Z", "")
    mode = result.get("peer_wire_security")
    mode_suffix = f"-{mode}" if mode is not None else ""
    return safe_artifact_id(
        f"{stamp}-{result['host_label']}-{result['transport']}{mode_suffix}-f{result['frames_per_connection']}"
    )


def campaign_id(result: dict[str, Any]) -> str | None:
    """Return the validated campaign label, excluding pre-campaign history."""
    value = result.get("_report_campaign_id", result.get("campaign_id"))
    return value if isinstance(value, str) and CAMPAIGN_ID.fullmatch(value) else None


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


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def historical_imports(report_dir: Path) -> dict[str, dict[str, str]]:
    """Load hash-bound display metadata for immutable historical artifacts."""
    manifest = report_dir / HISTORICAL_IMPORTS_NAME
    if not manifest.exists():
        return {}
    try:
        payload = json.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise BenchmarkReportError(f"{manifest}: invalid historical-import manifest") from error
    if not isinstance(payload, dict) or payload.get("schema_version") != 1:
        raise BenchmarkReportError(f"{manifest}: unsupported historical-import manifest")
    entries = payload.get("imports")
    if not isinstance(entries, list):
        raise BenchmarkReportError(f"{manifest}: imports must be a list")
    imported: dict[str, dict[str, str]] = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise BenchmarkReportError(f"{manifest}: import must be an object")
        filename = entry.get("filename")
        digest = entry.get("sha256")
        identifier = entry.get("campaign_id")
        display_host = entry.get("display_host_label")
        note = entry.get("provenance_note")
        if (
            not isinstance(filename, str) or Path(filename).name != filename or not filename.endswith(".json")
            or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest)
            or not isinstance(identifier, str) or not CAMPAIGN_ID.fullmatch(identifier)
            or not isinstance(display_host, str) or not SAFE_ID.fullmatch(display_host)
            or not isinstance(note, str) or not note.strip()
        ):
            raise BenchmarkReportError(f"{manifest}: invalid historical import entry")
        artifact = report_dir / filename
        if not artifact.is_file() or file_sha256(artifact) != digest:
            raise BenchmarkReportError(f"{manifest}: hash mismatch for {filename}")
        if filename in imported:
            raise BenchmarkReportError(f"{manifest}: duplicate import for {filename}")
        imported[filename] = {
            "campaign_id": identifier,
            "display_host_label": display_host,
            "provenance_note": note.strip(),
        }
    return imported


def evidence_records(report_dir: Path = REPORT_DIR) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    if not report_dir.is_dir():
        return records
    imported = historical_imports(report_dir)
    for path in sorted(report_dir.glob("*.json")):
        if (
            path.name.endswith(".envelope.json")
            or path.name.endswith(".campaign.json")
            or path.name in {HISTORICAL_IMPORTS_NAME, BASELINES_PATH.name}
        ):
            continue
        try:
            record = load_result(path)
            if metadata := imported.get(path.name):
                record.update({f"_report_{key}": value for key, value in metadata.items()})
            records.append(record)
        except BenchmarkReportError:
            continue
    return sorted(records, key=lambda item: (item["generated_at"], item["host_label"], item["transport"], item["frames_per_connection"]))


def render_run(result: dict[str, Any], artifact_id: str, report_dir: Path = REPORT_DIR) -> Path:
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
            "transport": result["transport"],
            "peer_wire_security": result.get("peer_wire_security") or "legacy-unverified",
            "benchmark_target": result.get("benchmark_target") or "legacy",
            "hook_mode": result.get("hook_mode") or "unknown",
            "daemon_version": result.get("daemon_version") or "unknown",
            "host_os": result.get("host_os") or "unknown",
            "host_arch": result.get("host_arch") or "unknown",
            "command": result.get("command") or "legacy",
            "execution_daemon": result.get("execution_daemon") or "legacy-unverified",
            "frames_per_connection": result["frames_per_connection"],
            "run_duration_s": result["run_duration_s"],
            "passed": result["passed"],
            "benchmark_evidence_failure_code": result.get("benchmark_evidence_failure_code", ""),
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
    latest: dict[tuple[str, str, str, int], dict[str, Any]] = {}
    for result in records:
        key = (
            result["host_label"],
            result["transport"],
            result.get("peer_wire_security") or "legacy-unverified",
            result["frames_per_connection"],
        )
        previous = latest.get(key)
        if previous is None or result["generated_at"] > previous["generated_at"]:
            latest[key] = result
    return list(latest.values())


def campaign_groups(records: Iterable[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    """Group post-AO2.7 evidence into immutable, versioned campaign reports."""
    groups: dict[str, list[dict[str, Any]]] = {}
    for result in records:
        identifier = campaign_id(result)
        if identifier is not None:
            groups.setdefault(identifier, []).append(result)
    return {
        identifier: sorted(group, key=lambda item: item["generated_at"])
        for identifier, group in groups.items()
    }


def load_baselines(path: Path = BASELINES_PATH) -> BaselineSet:
    """Load the sole reviewed acceptance floors used by benchmark reports."""
    try:
        return BaselineSet.model_validate_json(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise BenchmarkReportError(f"could not load benchmark baselines {path}: {error}") from error


def campaign_target_rows(
    records: Iterable[dict[str, Any]], baselines: BaselineSet | None = None,
) -> list[dict[str, Any]]:
    """Return one revision-homogeneous result/floor/verdict table."""
    records = list(records)
    revisions = {record.get("source_revision") for record in records}
    if len(revisions) > 1:
        raise BenchmarkReportError("campaign target table cannot mix source revisions")
    newest: dict[str, dict[str, Any]] = {}
    for result in records:
        target = result.get("benchmark_target")
        if target in TARGET_ORDER and (
            target not in newest or result["generated_at"] > newest[target]["generated_at"]
        ):
            newest[target] = result
    if baselines is None:
        baselines = load_baselines()
    rows: list[dict[str, Any]] = []
    for target in TARGET_ORDER:
        result = newest.get(target)
        baseline = None
        if result is not None:
            try:
                baseline = baselines.entry_for(result["host_label"], target)
            except BenchmarkSchemaError:
                baseline = None
        metrics = result.get("metrics") if result else None
        metric = metrics.get("admissions_per_second", {}) if isinstance(metrics, dict) else {}
        value = metric.get("p50") if isinstance(metric, dict) else None
        accepted = metrics.get("accepted_count") if isinstance(metrics, dict) else None
        requested = metrics.get("requested_count") if isinstance(metrics, dict) else None
        passed = (
            isinstance(value, (int, float))
            and isinstance(accepted, int)
            and accepted == requested
            and baseline is not None
            and float(value) >= baseline.p50_floor
            and result.get("status", "PASS") == "PASS"
        )
        rows.append({
            "test": target,
            "result_msg_per_second": None if value is None else float(value),
            "baseline_msg_per_second": None if baseline is None else baseline.p50_floor,
            "passed": passed,
            "artifact_id": result_id(result) if result else None,
            "json_href": f"{result_id(result)}.json" if result else None,
            "xhtml_href": f"{result_id(result)}.xhtml" if result else None,
        })
    return rows


def campaign_panels(records: Iterable[dict[str, Any]]) -> list[dict[str, Any]]:
    """Return one provenance-preserving panel per rendered host and revision."""
    by_host_revision: dict[tuple[str, str], list[dict[str, Any]]] = {}
    for result in records:
        source_revision = result.get("source_revision") or "unversioned"
        display_host = result.get("_report_display_host_label", result["host_label"])
        by_host_revision.setdefault((display_host, source_revision), []).append(result)
    host_panels = []
    for (host_label, source_revision), host_records in sorted(by_host_revision.items()):
        rows = campaign_target_rows(host_records)
        host_panels.append({
            "host_label": host_label,
            "source_revision": source_revision,
            "daemon_version": host_records[-1].get("daemon_version") or "unknown",
            "host_os": host_records[-1].get("host_os") or "unknown",
            "host_arch": host_records[-1].get("host_arch") or "unknown",
            "raw_host_label": host_records[-1]["host_label"],
            "provenance_note": host_records[-1].get("_report_provenance_note"),
            "rows": rows,
            "passed": all(row["passed"] for row in rows),
        })
    return host_panels


def render_campaign(identifier: str, records: Iterable[dict[str, Any]], report_dir: Path = REPORT_DIR) -> Path:
    """Render one date/version campaign as XHTML with one panel per host/revision."""
    host_panels = campaign_panels(records)
    report_dir.mkdir(parents=True, exist_ok=True)
    output = report_dir / f"{identifier}.xhtml"
    compose(
        ROOT / "templates" / "benchmark-report" / "benchmark-report.xhtml.j2",
        {
            "title": f"ATM benchmark campaign — {identifier}",
            "campaign_id": identifier,
            "generated_at": utc_now(),
            "host_panels": host_panels,
        },
        output,
    )
    return output


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
            if (
                result["host_label"],
                result["transport"],
                result.get("peer_wire_security") or "legacy-unverified",
            ) == (
                newest["host_label"],
                newest["transport"],
                newest.get("peer_wire_security") or "legacy-unverified",
            )
        ]
    key = (
        newest["host_label"],
        newest["transport"],
        newest.get("peer_wire_security") or "legacy-unverified",
        revision,
    )
    return [
        result
        for result in records
        if (
            result["host_label"],
            result["transport"],
            result.get("peer_wire_security") or "legacy-unverified",
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


def render_aggregate(records: Iterable[dict[str, Any]], report_root: Path = REPORTS_ROOT) -> Path:
    groups = campaign_groups(records)
    campaigns = []
    for identifier, campaign in sorted(groups.items(), reverse=True):
        host_labels = sorted({record["host_label"] for record in campaign})
        campaigns.append({
            "campaign_id": identifier,
            "host_labels": ", ".join(
                sorted({record.get("_report_display_host_label", record["host_label"]) for record in campaign})
            ),
            "source_revision": campaign[-1].get("source_revision") or "unversioned",
            "generated_at": campaign[-1]["generated_at"],
            "xhtml_href": f"{REPORT_NAME}/{identifier}.xhtml",
            "host_panels": campaign_panels(campaign),
        })
    output = report_root / REPORT_HTML
    template = ROOT / "templates" / "benchmark-report" / "benchmark-report.html.j2"
    compose(template, {
        "title": "ATM local transport benchmark", "generated_at": utc_now(),
        "campaigns": campaigns,
        "legacy_count": sum(campaign_id(result) is None for result in records),
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


def persist_result(result: dict[str, Any]) -> str:
    """Store one validated result under its canonical immutable identity."""
    artifact_id = result_id(result)
    artifact = json.dumps(result, indent=2, sort_keys=True) + "\n"
    immutable_write(REPORT_DIR / f"{artifact_id}.json", artifact)
    immutable_write(REPORT_DIR / f"{artifact_id}.envelope.json", envelope_for(result))
    return artifact_id


def persist(source: Path) -> tuple[dict[str, Any], str]:
    result = load_result(source)
    return result, persist_result(result)


def process(inputs: list[Path]) -> int:
    errors: list[str] = []
    if not inputs:
        try:
            records = evidence_records()
            for result in records:
                # Rebuild is a rendering operation: evidence is immutable and
                # must never be normalized, renamed, or rewritten merely to
                # regenerate HTML.  New inputs are persisted in the input
                # branch below before they enter this path.
                artifact_id = result_id(result)
                render_run(result, artifact_id)
                immutable_write(REPORT_DIR / f"{artifact_id}.envelope.json", envelope_for(result))
            for identifier, campaign in campaign_groups(records).items():
                render_campaign(identifier, campaign)
            render_aggregate(records)
            regenerate_index()
        except (BenchmarkReportError, OSError) as error:
            errors.append(str(error))
    for source in inputs:
        try:
            result, artifact_id = persist(source)
            render_run(result, artifact_id)
            records = evidence_records()
            for identifier, campaign in campaign_groups(records).items():
                render_campaign(identifier, campaign)
            render_aggregate(records)
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
