#!/usr/bin/env python3
"""Build the AO2.12 historical record without changing legacy evidence bytes."""
from __future__ import annotations

import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_schema import (
    BaselineEntry,
    BaselineRef,
    BaselineSet,
    BenchmarkCampaign,
    BenchmarkRunResult,
    BenchmarkSchemaError,
    BenchmarkSummary,
    HistoricalCampaignEntry,
    HistoricalRecord,
    HistoricalResultEntry,
    RatchetPoint,
    UnattributedEntry,
    classify_status,
    compact_evidence,
)


REPORT_DIR = ROOT / "site" / "reports" / "send-message-benchmark"
ARTIFACT_DIR = ROOT / "artifacts" / "benchmark"
ZERO_REVISION = "0" * 40


class MigrationError(ValueError):
    """A source artifact cannot be normalized without inventing evidence."""


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise MigrationError(f"{path.name}: invalid JSON") from exc
    if not isinstance(value, dict):
        raise MigrationError(f"{path.name}: expected JSON object")
    return value


def source_files(reports_dir: Path) -> list[Path]:
    """Select only legacy benchmark result JSON, never envelopes/current v4."""
    selected: list[Path] = []
    for path in sorted(reports_dir.glob("*.json")):
        if path.name in {"baselines.json", "historical-imports.json", "historical-record.json"}:
            continue
        payload = read_json(path)
        if payload.get("artifact_kind") != "send_message_benchmark_summary":
            continue
        version = payload.get("schema_version")
        if version not in {1, 2, 3} or isinstance(version, bool):
            raise MigrationError(f"{path.name}: unsupported legacy schema version {version!r}")
        selected.append(path)
    return selected


def legacy_summary(payload: dict[str, Any], source: Path) -> BenchmarkSummary:
    """Use the pre-existing compact summary validator without writing source."""
    version = payload.get("schema_version")
    candidate = dict(payload)
    if version in {1, 2} and "artifact_kind" not in candidate:
        samples = candidate.get("samples", candidate.get("runs", []))
        if not isinstance(samples, list):
            raise MigrationError(f"{source.name}: legacy samples must be a list")
        candidate.update({
            "schema_version": 2,
            "run_duration_s": candidate.get("run_duration_s", candidate.get("duration_seconds", 0)),
            "runs": samples if samples and isinstance(samples[0], dict) and "intervals" in samples[0] else [{"intervals": samples}],
            "minimum_sample_count": candidate.get("minimum_sample_count", len(samples)),
            "sample_count": candidate.get("sample_count", len(samples)),
            "target_duration_s": candidate.get("target_duration_s", candidate.get("run_duration_s", candidate.get("duration_seconds", 0))),
        })
        try:
            return compact_evidence(candidate)
        except BenchmarkSchemaError as exc:
            raise MigrationError(f"{source.name}: cannot compact legacy interval evidence: {exc}") from exc
    candidate["schema_version"] = 3
    try:
        return BenchmarkSummary.model_validate(candidate)
    except Exception as exc:
        raise MigrationError(f"{source.name}: invalid legacy summary: {exc}") from exc


def target_for(summary: BenchmarkSummary, source: Path) -> str:
    if summary.benchmark_target is not None:
        return summary.benchmark_target
    if summary.transport == "sqlite":
        return "sqlite"
    if summary.transport == "uds":
        return "uds"
    if summary.peer_wire_security == "mutual-tls":
        return "tcp-tls"
    if summary.transport == "tcp":
        return "tcp"
    raise MigrationError(f"{source.name}: cannot infer benchmark target")


def os_for(summary: BenchmarkSummary, source: Path) -> str:
    raw = (summary.host_os or "").lower()
    if raw in {"macos", "darwin"} or any(
        token in summary.host_label.lower() for token in ("mac", "m4", "m5", "rand", "local")
    ):
        return "macos"
    if raw in {"windows", "win32"} or "windows" in summary.host_label.lower():
        return "windows"
    if raw == "linux" or "linux" in summary.host_label.lower():
        return "linux"
    raise MigrationError(f"{source.name}: missing host OS prevents v4 migration")


def imports_by_filename(reports_dir: Path) -> dict[str, str]:
    path = reports_dir / "historical-imports.json"
    if not path.exists():
        return {}
    payload = read_json(path)
    values = payload.get("imports")
    if not isinstance(values, list):
        raise MigrationError("historical-imports.json: imports must be a list")
    mapping: dict[str, str] = {}
    for value in values:
        if not isinstance(value, dict) or not isinstance(value.get("filename"), str) or not isinstance(value.get("campaign_id"), str):
            raise MigrationError("historical-imports.json: each import requires filename and campaign_id")
        mapping[value["filename"]] = value["campaign_id"]
    return mapping


def fallback_campaign_id(source: Path) -> str:
    digest = hashlib.sha256(source.name.encode("utf-8")).hexdigest()[:12]
    return f"historical-{digest}"


def stable_revision(value: str | None) -> str:
    return value if value is not None else ZERO_REVISION


def source_sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def result_from_summary(
    summary: BenchmarkSummary,
    source: Path,
    campaign_id: str,
    p50_floor: float,
) -> tuple[BenchmarkRunResult, str | None]:
    target = target_for(summary, source)
    metrics = summary.metrics
    if target == "sqlite" and metrics is not None:
        # Older sqlite runs sometimes recorded zero-valued wire fields.  v4
        # represents their absence rather than changing the measured values.
        metrics = metrics.model_copy(update={
            "connection_count": None,
            "application_wire_bytes": None,
            "request_frames_per_second": None,
            "connections_per_second": None,
            "application_wire_bytes_per_second": None,
        })
    durability = summary.durability_after_restart
    gap = None if durability is not None else "durability-counts-missing"
    status = classify_status(
        lifecycle_complete=metrics is not None and durability is not None,
        messages_requested=0 if metrics is None else metrics.requested_count,
        messages_admitted=0 if metrics is None else metrics.accepted_count,
        messages_durable=0 if durability is None else durability.observed_mailbox_count,
        p50_admissions_per_second=None if metrics is None else metrics.admissions_per_second.p50,
        baseline_p50_floor=p50_floor,
    )
    reason = None
    if status == "INCOMPLETE":
        reason = str(summary.failure or "durability counts were not recorded by this legacy artifact")
    try:
        return BenchmarkRunResult(
            campaign_id=campaign_id,
            host_label=summary.host_label,
            os=os_for(summary, source),
            target=target,
            status=status,
            incomplete_reason=reason,
            generated_at=datetime.fromisoformat(summary.generated_at.replace("Z", "+00:00")),
            source_revision=stable_revision(summary.source_revision),
            binary_hashes={},
            frames_per_connection=0 if target == "sqlite" else summary.frames_per_connection,
            messages_requested=0 if metrics is None else metrics.requested_count,
            messages_admitted=0 if metrics is None else metrics.accepted_count,
            messages_durable=0 if durability is None else durability.observed_mailbox_count,
            metrics=metrics,
            baseline=BaselineRef(revision=1, p50_floor=p50_floor),
            durability_after_restart=durability,
            direct_sqlite_message_write=summary.direct_sqlite_message_write,
        ), gap
    except Exception as exc:
        raise MigrationError(f"{source.name}: cannot make v4 result: {exc}") from exc


def migrated_record(reports_dir: Path, generated_from_commit: str) -> tuple[HistoricalRecord, dict[str, Any]]:
    imports = imports_by_filename(reports_dir)
    facts: list[tuple[Path, BenchmarkSummary, str, str]] = []
    for path in source_files(reports_dir):
        summary = legacy_summary(read_json(path), path)
        campaign = summary.campaign_id or imports.get(path.name) or fallback_campaign_id(path)
        facts.append((path, summary, campaign, target_for(summary, path)))
    facts.sort(key=lambda fact: (fact[1].generated_at, fact[0].name))

    best: dict[tuple[str, str], float] = {}
    converted: list[tuple[Path, BenchmarkRunResult, str | None]] = []
    ratchet: list[RatchetPoint] = []
    for path, summary, campaign, target in facts:
        key = (summary.host_label, target)
        prior = best.get(key, 0.0)
        metrics = summary.metrics
        durable = summary.durability_after_restart
        eligible = bool(
            metrics is not None and durable is not None and durable.passed
            and durable.observed_mailbox_count == durable.expected_accepted_count
        )
        observed = None if metrics is None else metrics.admissions_per_second.p50
        floor = max(prior, observed) if eligible and observed is not None else prior
        result, gap = result_from_summary(summary, path, campaign, floor)
        if eligible and observed is not None and floor > prior:
            best[key] = floor
            ratchet.append(RatchetPoint(
                host_label=summary.host_label, target=target,
                effective_from=result.generated_at, p50_floor=floor,
                source_campaign_id=campaign,
            ))
        converted.append((path, result, gap))

    by_campaign: dict[str, list[tuple[Path, BenchmarkRunResult, str | None]]] = defaultdict(list)
    for item in converted:
        by_campaign[item[1].campaign_id].append(item)
    entries: list[HistoricalCampaignEntry] = []
    for campaign_id, items in by_campaign.items():
        items.sort(key=lambda item: (item[1].generated_at, item[0].name))
        first = items[0][1]
        unique = {item[1].target for item in items}
        if len(unique) != len(items):
            # A legacy frame sweep has repeated targets.  Preserve each source
            # record in an individual incomplete historical campaign rather
            # than dropping or coalescing its measured value.
            for path, result, gap in items:
                single = result.model_copy(update={"campaign_id": fallback_campaign_id(path)})
                campaign = BenchmarkCampaign(
                    campaign_id=single.campaign_id, host_label=single.host_label, os=single.os,
                    phase="historical", started_at=single.generated_at, completed_at=single.generated_at,
                    source_revision=single.source_revision, results=(single,), status="INCOMPLETE",
                )
                entries.append(HistoricalCampaignEntry(
                    campaign=campaign, final_best=True,
                    results=(HistoricalResultEntry(result=single, displayed_status=("INCOMPLETE" if gap else single.status), evidence_gap=gap, source_files=(path.name,)),),
                ))
            continue
        campaign_status = classify_status(
            required_targets={"sqlite", "tcp", "tcp-tls"} | ({"uds"} if first.os != "windows" else set()),
            observed_targets={item[1].target for item in items},
            target_statuses=tuple(item[1].status for item in items),
        )
        campaign = BenchmarkCampaign(
            campaign_id=campaign_id, host_label=first.host_label, os=first.os,
            phase="historical", started_at=min(item[1].generated_at for item in items),
            completed_at=max(item[1].generated_at for item in items), source_revision=first.source_revision,
            results=tuple(item[1] for item in items), status=campaign_status,
        )
        entries.append(HistoricalCampaignEntry(
            campaign=campaign, final_best=True,
            results=tuple(HistoricalResultEntry(
                result=result, displayed_status=("INCOMPLETE" if gap else result.status), evidence_gap=gap,
                source_files=(path.name,),
            ) for path, result, gap in items),
        ))
    entries.sort(key=lambda entry: entry.campaign.started_at)
    record = HistoricalRecord(
        schema_version=1, generated_from_commit=generated_from_commit,
        campaigns=tuple(entries), ratchet=tuple(ratchet), unattributed=(),
    )
    audit = {
        "schema_version": 1,
        "generated_from_commit": generated_from_commit,
        "source_count": len(facts),
        "unattributed_count": 0,
        "mappings": [
            {
                "source_file": path.name, "source_sha256": source_sha(path),
                "campaign_id": result.campaign_id, "target": result.target,
                "generated_at": summary.generated_at,
                "source_revision": summary.source_revision,
                "metrics": None if summary.metrics is None else summary.metrics.model_dump(mode="json"),
                "counts": None if summary.metrics is None else {
                    "requested": summary.metrics.requested_count,
                    "accepted": summary.metrics.accepted_count,
                    "response": summary.metrics.response_count,
                },
            }
            for path, summary, _campaign, _target in facts
            for result in [next(item[1] for item in converted if item[0] == path)]
        ],
    }
    return record, audit


def write_if_changed(path: Path, content: str) -> bool:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and path.read_text(encoding="utf-8") == content:
        return False
    path.write_text(content, encoding="utf-8")
    return True


def rendered_audit(audit: dict[str, Any]) -> str:
    rows = "\n".join(
        f"<tr><td>{html.escape(item['source_file'])}</td><td>{html.escape(item['campaign_id'])}</td>"
        f"<td>{html.escape(item['target'])}</td><td>{html.escape(item['generated_at'])}</td></tr>"
        for item in audit["mappings"]
    )
    return (
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Benchmark migration audit</title></head>"
        "<body><h1>Benchmark migration audit</h1><table><thead><tr><th>Source</th><th>Campaign</th>"
        f"<th>Target</th><th>Recorded UTC</th></tr></thead><tbody>{rows}</tbody></table></body></html>\n"
    )


def updated_baselines(reports_dir: Path, record: HistoricalRecord) -> BaselineSet:
    """Append Windows-only historical seeds, never revise an existing floor."""
    path = reports_dir / "baselines.json"
    try:
        current = BaselineSet.model_validate(read_json(path))
    except Exception as exc:
        raise MigrationError(f"baselines.json: invalid baseline set: {exc}") from exc
    present = {(entry.host_label, entry.target) for entry in current.entries}
    best: dict[tuple[str, str], tuple[float, datetime, str]] = {}
    for campaign_entry in record.campaigns:
        for entry in campaign_entry.results:
            result = entry.result
            if result.os != "windows" or entry.displayed_status != "PASS" or result.metrics is None:
                continue
            key = (result.host_label, result.target)
            candidate = (result.metrics.admissions_per_second.p50, result.generated_at, result.campaign_id)
            if key not in best or candidate[0] > best[key][0]:
                best[key] = candidate
    additions = tuple(
        BaselineEntry(
            host_label=host, target=target, p50_floor=value,
            approved_by="historical migration seed; pending quality review",
            effective_from=timestamp,
        )
        for (host, target), (value, timestamp, _campaign) in sorted(best.items())
        if (host, target) not in present
    )
    return BaselineSet(
        schema_version=1,
        revision=current.revision if not additions else current.revision + 1,
        entries=current.entries + additions,
    )


def current_revision() -> str:
    completed = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, check=True, capture_output=True, text=True)
    return completed.stdout.strip()


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reports-dir", type=Path, default=REPORT_DIR)
    parser.add_argument("--check", action="store_true", help="validate and compare without writing")
    args = parser.parse_args(argv)
    reports_dir = args.reports_dir.resolve()
    record, audit = migrated_record(reports_dir, current_revision())
    record_path = reports_dir / "historical-record.json"
    baselines_path = reports_dir / "baselines.json"
    audit_json = ARTIFACT_DIR / "migration-audit.json"
    audit_html = ARTIFACT_DIR / "migration-audit.html"
    record_text = record.model_dump_json(indent=2) + "\n"
    baseline_text = updated_baselines(reports_dir, record).model_dump_json(indent=2) + "\n"
    audit_text = json.dumps(audit, indent=2, sort_keys=True) + "\n"
    if args.check:
        if not record_path.exists():
            raise MigrationError(f"{record_path}: missing; run without --check first")
        if record_path.read_text(encoding="utf-8") != record_text:
            raise MigrationError(f"{record_path}: differs from deterministic migration output")
        if baselines_path.read_text(encoding="utf-8") != baseline_text:
            raise MigrationError(f"{baselines_path}: differs from deterministic Windows seed output")
        return 0
    write_if_changed(record_path, record_text)
    write_if_changed(baselines_path, baseline_text)
    write_if_changed(audit_json, audit_text)
    write_if_changed(audit_html, rendered_audit(audit))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except MigrationError as exc:
        print(f"benchmark history migration failed: {exc}", file=sys.stderr)
        raise SystemExit(2)
