#!/usr/bin/env python3
"""Deterministically render the public benchmark report site from v4 JSON."""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Iterable, Literal, Sequence
from zoneinfo import ZoneInfo


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_schema import (
    BaselineSet,
    BenchmarkCampaign,
    BenchmarkRunResult,
    BenchmarkSchemaError,
    HistoricalRecord,
    artifact_id,
    classify_status,
)


REPORTS_ROOT = ROOT / "site" / "reports"
REPORT_NAME = "send-message-benchmark"
REPORT_DIR = REPORTS_ROOT / REPORT_NAME
HISTORICAL_RECORD_NAME = "historical-record.json"
BASELINES_FILENAME = "baselines.json"
TARGET_ORDER: tuple[Literal["sqlite", "uds", "tcp", "tcp-tls"], ...] = (
    "tcp", "tcp-tls", "uds", "sqlite",
)
TARGET_LABELS = {"sqlite": "SQLite", "uds": "UDS", "tcp": "TCP", "tcp-tls": "TCP + TLS"}
SERIES_COLORS = ("#2563eb", "#9333ea", "#0f766e", "#c2410c", "#475569")
PACIFIC = ZoneInfo("America/Los_Angeles")
SAFE_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")


class BenchmarkReportError(ValueError):
    """The public benchmark evidence cannot be deterministically rendered."""


def _safe_artifact_id(value: str) -> str:
    candidate = re.sub(r"[^A-Za-z0-9._-]+", "-", value).strip("-")[:128]
    if not candidate or not SAFE_ID.fullmatch(candidate):
        raise BenchmarkReportError(f"unsafe benchmark artifact id: {value!r}")
    return candidate


def utc_text(value: datetime) -> str:
    """Serialize an already-validated UTC timestamp in the public wire form."""
    return value.astimezone(timezone.utc).isoformat().replace("+00:00", "Z")


def time_view(value: datetime) -> dict[str, str]:
    """Shared UTC/Pacific presentation helper used by every template context."""
    utc_value = value.astimezone(timezone.utc)
    local = utc_value.astimezone(PACIFIC)
    # `%-d` is a glibc/macOS strftime extension rejected by Windows' CRT;
    # interpolate the day number portably instead.
    return {
        "datetime": utc_text(utc_value),
        "text": local.strftime(f"%b {local.day}, %Y · %H:%M %Z"),
    }


def target_label(target: str) -> str:
    try:
        return TARGET_LABELS[target]
    except KeyError as exc:
        raise BenchmarkReportError(f"unknown benchmark target: {target!r}") from exc


def compose(template: Path, variables: dict[str, Any], output: Path) -> None:
    """Render a checked-in sc-compose template without network dependencies."""
    output.parent.mkdir(parents=True, exist_ok=True)
    variables_path: Path | None = None
    try:
        with tempfile.NamedTemporaryFile("w", suffix=".json", encoding="utf-8", delete=False) as handle:
            json.dump(variables, handle, sort_keys=True)
            variables_path = Path(handle.name)
        completed = subprocess.run(
            ["sc-compose", "render", "--root", str(ROOT), "--file", str(template),
             "--var-file", str(variables_path), "--output", str(output)],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace", check=False,
        )
        if completed.returncode != 0:
            raise BenchmarkReportError(
                f"sc-compose render failed: {completed.stderr.strip() or completed.stdout.strip()}"
            )
    finally:
        if variables_path is not None:
            variables_path.unlink(missing_ok=True)


def load_json(path: Path) -> dict[str, Any]:
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise BenchmarkReportError(f"{path}: invalid JSON") from exc
    if not isinstance(payload, dict):
        raise BenchmarkReportError(f"{path}: JSON root must be an object")
    return payload


def load_result(source: Path) -> dict[str, Any]:
    """Read one strict v4 benchmark result; historical loading is retired."""
    payload = load_json(source)
    try:
        result = BenchmarkRunResult.model_validate(payload).model_dump(mode="json")
    except Exception as exc:  # pydantic reports structured validation details.
        raise BenchmarkReportError(f"{source}: invalid v4 benchmark result: {exc}") from exc
    target = result["target"]
    return {
        **result,
        "transport": "sqlite" if target == "sqlite" else ("uds" if target == "uds" else "tcp"),
        "peer_wire_security": (
            None if target == "sqlite" else ("plaintext-test" if target == "tcp" else "mutual-tls")
        ),
        "benchmark_target": target,
        "passed": result["status"] == "PASS",
    }


def result_id(result: dict[str, Any], _source: Path | None = None) -> str:
    """Return the existing immutable artifact identifier without creating output."""
    if result.get("schema_version") == 4:
        try:
            return artifact_id(campaign_id=str(result["campaign_id"]), target=str(result["target"]))
        except BenchmarkSchemaError as exc:
            raise BenchmarkReportError(f"invalid v4 artifact identity: {exc}") from exc
    stamp = str(result["generated_at"]).replace("-", "").replace(":", "").replace("T", "-").replace("Z", "")
    security = result.get("peer_wire_security")
    suffix = f"-{security}" if security is not None else ""
    return _safe_artifact_id(
        f"{stamp}-{result['host_label']}-{result['transport']}{suffix}-f{result['frames_per_connection']}"
    )


def load_campaigns(report_dir: Path = REPORT_DIR) -> list[BenchmarkCampaign]:
    """Read only validated immutable v4 campaign JSON, ordered by UTC start."""
    campaigns: list[BenchmarkCampaign] = []
    for path in sorted(report_dir.glob("*.campaign.json")):
        try:
            campaigns.append(BenchmarkCampaign.model_validate(load_json(path)))
        except (BenchmarkSchemaError, ValueError) as exc:
            raise BenchmarkReportError(f"{path}: invalid benchmark campaign: {exc}") from exc
    return sorted(campaigns, key=lambda campaign: campaign.started_at)


def empty_historical_record() -> HistoricalRecord:
    """The pre-AO2.12 fixture shape; no special-case ad-hoc historical data."""
    return HistoricalRecord(
        schema_version=1, generated_from_commit="0" * 40,
        campaigns=(), ratchet=(), unattributed=(),
    )


def load_historical_record(report_dir: Path = REPORT_DIR) -> HistoricalRecord:
    path = report_dir / HISTORICAL_RECORD_NAME
    if not path.exists():
        return empty_historical_record()
    try:
        return HistoricalRecord.model_validate(load_json(path))
    except (BenchmarkSchemaError, ValueError) as exc:
        raise BenchmarkReportError(f"{path}: invalid historical record: {exc}") from exc


def load_baselines(report_dir: Path = REPORT_DIR) -> BaselineSet:
    path = report_dir / BASELINES_FILENAME
    try:
        return BaselineSet.model_validate(load_json(path))
    except (BenchmarkSchemaError, ValueError) as exc:
        raise BenchmarkReportError(f"{path}: invalid baseline set: {exc}") from exc


def campaign_rows(campaign: BenchmarkCampaign) -> list[dict[str, Any]]:
    """Create the fixed target matrix directly from immutable result snapshots."""
    by_target = {result.target: result for result in campaign.results}
    rows: list[dict[str, Any]] = []
    for target in TARGET_ORDER:
        result = by_target.get(target)
        p50 = None if result is None or result.metrics is None else result.metrics.admissions_per_second.p50
        p95 = None if result is None or result.metrics is None else result.metrics.admissions_per_second.p95
        p99 = None if result is None or result.metrics is None else result.metrics.admissions_per_second.p99
        durable = None if result is None else result.durability_after_restart
        rows.append({
            "target": target, "label": target_label(target),
            "p50": p50, "p95": p95, "p99": p99,
            "baseline": None if result is None else result.baseline.p50_floor,
            "margin": None if p50 is None or result is None else p50 - result.baseline.p50_floor,
            "status": "INCOMPLETE" if result is None else result.status,
            "durable": None if durable is None else durable.passed,
            "durable_counts": "not captured" if durable is None else (
                f"{durable.observed_mailbox_count} / {durable.expected_accepted_count} after restart"
            ),
        })
    return rows


def incomplete_reason(campaign: BenchmarkCampaign) -> str | None:
    if campaign.status != "INCOMPLETE":
        return None
    reasons = [result.incomplete_reason for result in campaign.results if result.incomplete_reason]
    return "; ".join(reasons) if reasons else "Required target results are missing."


def current_historical_display_status(
    result: BenchmarkRunResult, historical: HistoricalRecord,
) -> str:
    """Reclassify a historical point against the ratchet's current high-water mark.

    ``result.status`` and its baseline are immutable ingest-time evidence.  A
    phase chart instead answers the distinct, present-tense question: does this
    older point meet the best durable result subsequently observed for this
    host/target?  The distinction is temporal, never a manufactured floor.
    """
    current_floor = max(
        (
            point.p50_floor
            for point in historical.ratchet
            if point.host_label == result.host_label and point.target == result.target
        ),
        default=result.baseline.p50_floor,
    )
    return classify_status(
        lifecycle_complete=result.metrics is not None and result.durability_after_restart is not None,
        messages_requested=result.messages_requested,
        messages_admitted=result.messages_admitted,
        messages_durable=result.messages_durable,
        p50_admissions_per_second=(
            None if result.metrics is None else result.metrics.admissions_per_second.p50
        ),
        baseline_p50_floor=current_floor,
    )


def panel_variables(campaign: BenchmarkCampaign) -> dict[str, Any]:
    return {
        "title": f"ATM benchmark campaign — {campaign.campaign_id}",
        "campaign_id": campaign.campaign_id,
        "host_label": campaign.host_label,
        "phase": campaign.phase,
        "source_revision": campaign.source_revision,
        "status": campaign.status,
        "started_at": time_view(campaign.started_at),
        "completed_at": None if campaign.completed_at is None else time_view(campaign.completed_at),
        "rows": campaign_rows(campaign),
        "incomplete_reason": incomplete_reason(campaign),
    }


def phase_slug(phase: str) -> str:
    clean = "".join(character.lower() if character.isalnum() else "-" for character in phase).strip("-")
    if not clean:
        raise BenchmarkReportError(f"unsafe empty phase label: {phase!r}")
    return f"phase-{clean}.html"


def _chart_points(
    target: str,
    historical: HistoricalRecord,
    phase_campaigns: Sequence[BenchmarkCampaign],
) -> list[dict[str, Any]]:
    points: list[dict[str, Any]] = []
    for entry in historical.campaigns:
        if not entry.final_best:
            continue
        for historical_result in entry.results:
            result = historical_result.result
            status = current_historical_display_status(result, historical)
            if result.target == target and result.metrics is not None and status != "INCOMPLETE":
                points.append({
                    "host_label": result.host_label, "timestamp": result.generated_at,
                    "status": status,
                    "distribution": result.metrics.admissions_per_second,
                })
    for campaign in phase_campaigns:
        if campaign.status == "INCOMPLETE":
            continue
        for result in campaign.results:
            if result.target == target and result.metrics is not None:
                points.append({
                    "host_label": result.host_label, "timestamp": campaign.started_at,
                    "status": result.status, "distribution": result.metrics.admissions_per_second,
                })
    return sorted(points, key=lambda point: (point["timestamp"], point["host_label"]))


def candlestick_series(
    charts: Sequence[Literal["tcp", "tcp-tls", "uds", "sqlite"]],
    historical: HistoricalRecord,
    phase_campaigns: Sequence[BenchmarkCampaign],
    baselines: BaselineSet,
) -> dict[str, dict[str, Any]]:
    """Return pure JSON-serializable SVG geometry; templates never do chart math."""
    result: dict[str, dict[str, Any]] = {}
    for target in charts:
        points = _chart_points(target, historical, phase_campaigns)
        labels = sorted({point["host_label"] for point in points})
        colors = {label: SERIES_COLORS[index % len(SERIES_COLORS)] for index, label in enumerate(labels)}
        floors = {
            label: entry.p50_floor
            for label in labels
            for entry in baselines.entries
            if entry.host_label == label and entry.target == target
        }
        values = [value for point in points for value in (
            point["distribution"].min, point["distribution"].max,
        )] + list(floors.values())
        lower, upper = (min(values), max(values)) if values else (0.0, 1.0)
        if lower == upper:
            lower, upper = 0.0, upper + 1.0
        y = lambda value: round(200 - ((value - lower) / (upper - lower) * 160), 3)
        count = max(len(points), 1)
        candles: list[dict[str, Any]] = []
        for index, point in enumerate(points):
            distribution = point["distribution"]
            candles.append({
                "host_label": point["host_label"], "color": colors[point["host_label"]],
                "status": point["status"], "x": round(50 + (index + 0.5) * (500 / count), 3),
                "low": y(distribution.min), "high": y(distribution.max),
                "p50": y(distribution.p50), "p95": y(distribution.p95),
                "timestamp": time_view(point["timestamp"]),
            })
        result[target] = {
            "target": target, "label": target_label(target), "series": [
                {"host_label": label, "color": colors[label]} for label in labels
            ], "candles": candles,
            "baseline_lines": [
                {"host_label": label, "y": y(floor), "floor": floor, "color": colors[label]}
                for label, floor in sorted(floors.items())
            ],
            "axis_min": lower, "axis_max": upper,
        }
    return result


def phase_variables(
    phase: str,
    campaigns: Sequence[BenchmarkCampaign],
    historical: HistoricalRecord,
    baselines: BaselineSet,
) -> dict[str, Any]:
    charts = candlestick_series(TARGET_ORDER, historical, campaigns, baselines)
    return {
        "title": f"ATM benchmark phase report — {phase}", "phase": phase,
        "charts": [charts[target] for target in TARGET_ORDER],
        "campaigns": [panel_variables(campaign) for campaign in sorted(campaigns, key=lambda item: item.started_at, reverse=True)],
    }


def render_panel(campaign: BenchmarkCampaign, report_dir: Path = REPORT_DIR) -> Path:
    output = report_dir / f"{campaign.campaign_id}.xhtml"
    compose(ROOT / "templates/benchmark-report/benchmark-run.xhtml.j2", panel_variables(campaign), output)
    return output


def render_phase(
    phase: str, campaigns: Sequence[BenchmarkCampaign], historical: HistoricalRecord,
    baselines: BaselineSet, report_dir: Path = REPORT_DIR,
) -> Path:
    output = report_dir / phase_slug(phase)
    compose(ROOT / "templates/benchmark-report/benchmark-phase-report.html.j2",
            phase_variables(phase, campaigns, historical, baselines), output)
    return output


def render_index(
    phase_groups: dict[str, list[BenchmarkCampaign]], historical: HistoricalRecord,
    baselines: BaselineSet, report_dir: Path = REPORT_DIR,
) -> Path:
    latest_phase, latest_campaigns = max(
        phase_groups.items(), key=lambda item: max(campaign.started_at for campaign in item[1]),
    )
    latest = phase_variables(latest_phase, latest_campaigns, historical, baselines)
    phases = sorted(
        ((phase, campaigns) for phase, campaigns in phase_groups.items()),
        key=lambda item: max(campaign.started_at for campaign in item[1]), reverse=True,
    )
    output = report_dir / "index.html"
    compose(ROOT / "templates/benchmark-report/benchmark-index.html.j2", {
        "title": "ATM benchmark reports", "latest": latest,
        "phases": [{"phase": phase, "href": phase_slug(phase),
                    "started_at": time_view(max(campaign.started_at for campaign in campaigns))}
                   for phase, campaigns in phases],
    }, output)
    return output


def render_envelope(campaigns: Sequence[BenchmarkCampaign], report_root: Path = REPORTS_ROOT) -> None:
    latest = max(campaigns, key=lambda campaign: campaign.started_at)
    payload = {
        "schema_version": 1, "report_type": "benchmark",
        "generated_at": utc_text(latest.started_at), "host_label": latest.host_label,
        "report_html": f"{REPORT_NAME}/index.html",
    }
    (report_root / f"{REPORT_NAME}.json").write_text(
        json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def regenerate_index() -> None:
    completed = subprocess.run(
        ["just", "reports-index"], cwd=ROOT, capture_output=True, text=True,
        encoding="utf-8", errors="replace", check=False,
    )
    if completed.returncode != 0:
        raise BenchmarkReportError(f"reports-index failed: {completed.stderr.strip() or completed.stdout.strip()}")


def rebuild(report_dir: Path = REPORT_DIR, report_root: Path = REPORTS_ROOT, *, invoke_index: bool = True) -> list[Path]:
    """The sole render flow: validated JSON → panels → phases → index → indexer."""
    campaigns = load_campaigns(report_dir)
    if not campaigns:
        raise BenchmarkReportError(f"{report_dir}: no validated *.campaign.json files to render")
    historical, baselines = load_historical_record(report_dir), load_baselines(report_dir)
    phase_groups: dict[str, list[BenchmarkCampaign]] = {}
    for campaign in campaigns:
        phase_groups.setdefault(campaign.phase, []).append(campaign)
    outputs = [render_panel(campaign, report_dir) for campaign in campaigns]
    outputs.extend(render_phase(phase, group, historical, baselines, report_dir) for phase, group in phase_groups.items())
    outputs.append(render_index(phase_groups, historical, baselines, report_dir))
    # The prior aggregate is generated output, not immutable evidence.  Keep
    # historical per-run envelopes untouched; the indexer recognizes the new
    # canonical directory index and ignores those superseded sidecars.
    (report_root / f"{REPORT_NAME}.html").unlink(missing_ok=True)
    render_envelope(campaigns, report_root)
    if invoke_index:
        regenerate_index()
    return outputs


def preview_latest(report_dir: Path = REPORT_DIR, preview_root: Path = ROOT / "artifacts/benchmark/preview", *, open_viewer: bool = True) -> Path:
    campaigns = load_campaigns(report_dir)
    if not campaigns:
        raise BenchmarkReportError(f"{report_dir}: no campaign available for preview")
    newest = max(campaigns, key=lambda campaign: campaign.started_at)
    source = report_dir / f"{newest.campaign_id}.xhtml"
    if not source.is_file():
        raise BenchmarkReportError(f"{source}: rebuild reports before previewing")
    preview_root.mkdir(parents=True, exist_ok=True)
    output = preview_root / "latest.html"
    shutil.copyfile(source, output)
    if open_viewer:
        completed = subprocess.run(["wyvern", str(output)], cwd=ROOT, check=False)
        if completed.returncode != 0:
            raise BenchmarkReportError(f"wyvern failed with exit code {completed.returncode}")
    return output


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rebuild", action="store_true", help="rebuild report output from validated JSON (default)")
    args = parser.parse_args(argv)
    try:
        rebuild()
    except (BenchmarkReportError, OSError) as exc:
        print(f"benchmark-report: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
