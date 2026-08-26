from __future__ import annotations

from datetime import datetime, timedelta, timezone
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.benchmark_schema import (
    BaselineRef, BenchmarkCampaign, BenchmarkMetrics, BenchmarkRunResult,
    DurabilityAfterRestart, HistoricalRecord, MetricDistribution, RatchetPoint, WireBytes,
)


SCRIPT = ROOT / "scripts/smoke/benchmark_report.py"
spec = importlib.util.spec_from_file_location("benchmark_report", SCRIPT)
assert spec and spec.loader
REPORT = importlib.util.module_from_spec(spec)
spec.loader.exec_module(REPORT)


UTC = timezone.utc


def distribution(value: float) -> MetricDistribution:
    spread = min(300, value / 2)
    return MetricDistribution(min=value - spread, p50=value, p95=value + spread, p99=value + spread * 1.25, max=value + spread * 1.5)


def result(target: str, stamp: datetime, *, identifier: str, status: str = "PASS", host: str = "rand-m5", floor: float = 16_000, p50: float = 18_000) -> BenchmarkRunResult:
    network = target != "sqlite"
    metrics = BenchmarkMetrics(
        interval_count=2, passed_interval_count=2, accepted_count=10, requested_count=10,
        response_count=10, admissions_per_second=distribution(p50),
        time_to_send_1k_s=distribution(1), interval_latency_ms=distribution(1),
        **({"connection_count": 2, "application_wire_bytes": WireBytes(request=1, response=1, total=2),
            "request_frames_per_second": distribution(1), "connections_per_second": distribution(1),
            "application_wire_bytes_per_second": distribution(2)} if network else {}),
    )
    complete = status != "INCOMPLETE"
    effective_floor = 19_000 if status == "FAIL" else floor
    return BenchmarkRunResult(
        campaign_id=identifier, host_label=host, os="macos", target=target,
        status=status, incomplete_reason=None if complete else "daemon did not become ready",
        generated_at=stamp, source_revision="a" * 40, binary_hashes={"atm-daemon": "b" * 64},
        frames_per_connection=0 if target == "sqlite" else 8, messages_requested=10,
        messages_admitted=10, messages_durable=10, metrics=metrics if complete else None,
        baseline=BaselineRef(revision=1, p50_floor=effective_floor),
        durability_after_restart=DurabilityAfterRestart(expected_accepted_count=10, observed_mailbox_count=10, passed=True) if complete else None,
    )


def campaign(stamp: datetime, *, identifier: str, host: str = "rand-m5", status: str = "PASS", phase: str = "AO2") -> BenchmarkCampaign:
    results = tuple(result(target, stamp, identifier=identifier, host=host, status=status) for target in ("sqlite", "uds", "tcp", "tcp-tls"))
    return BenchmarkCampaign(
        campaign_id=identifier, host_label=host, os="macos", phase=phase, started_at=stamp,
        completed_at=stamp + timedelta(minutes=1), source_revision="a" * 40,
        results=results, status=status,
    )


def write_inputs(directory: Path, campaigns: list[BenchmarkCampaign]) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    for item in campaigns:
        (directory / f"{item.campaign_id}.campaign.json").write_text(item.model_dump_json(indent=2), encoding="utf-8")
    entries = [
        {"host_label": host, "target": target, "p50_floor": 16_000, "approved_by": "quality-mgr", "effective_from": "2026-08-01T00:00:00Z"}
        for host in sorted({item.host_label for item in campaigns}) for target in ("sqlite", "uds", "tcp", "tcp-tls")
    ]
    (directory / "baselines.json").write_text(json.dumps({"schema_version": 1, "revision": 1, "entries": entries}), encoding="utf-8")


class BenchmarkReportTests(unittest.TestCase):
    def test_time_view_is_pacific_with_utc_machine_value(self) -> None:
        rendered = REPORT.time_view(datetime(2026, 8, 24, 7, 59, tzinfo=UTC))
        self.assertEqual(rendered["datetime"], "2026-08-24T07:59:00Z")
        self.assertEqual(rendered["text"], "Aug 24, 2026 · 00:59 PDT")

    def test_empty_historical_record_is_the_shared_model(self) -> None:
        self.assertEqual(REPORT.empty_historical_record().campaigns, ())
        self.assertEqual(REPORT.empty_historical_record().schema_version, 1)

    def test_compatibility_reader_never_rewrites_recorded_result_bytes(self) -> None:
        item = result(
            "tcp", datetime(2026, 8, 24, 7, tzinfo=UTC),
            identifier="20260824T070000Z-rand-m5",
        )
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "recorded-result.json"
            source.write_text(item.model_dump_json(indent=2), encoding="utf-8")
            before = source.read_bytes()
            rendered = REPORT.load_result(source)
            after = source.read_bytes()
        self.assertEqual(after, before)
        self.assertEqual(rendered["target"], "tcp")
        self.assertEqual(rendered["status"], "PASS")

    def test_rebuild_renders_panels_phases_index_and_is_byte_identical(self) -> None:
        # Filename order is deliberately newest, oldest, middle: rendering
        # must use the UTC values rather than filenames or Pacific dates.
        # ``second`` and ``first`` share the Aug 23 Pacific calendar date,
        # while their UTC dates differ.  Filename order deliberately puts the
        # older record first, so a Pacific-date-only sort cannot satisfy the
        # descending UTC order asserted below.
        first = campaign(datetime(2026, 8, 24, 0, 30, tzinfo=UTC), identifier="z-later-utc")
        second = campaign(datetime(2026, 8, 23, 23, 30, tzinfo=UTC), identifier="a-earlier-utc")
        third = campaign(datetime(2026, 8, 24, 7, 1, tzinfo=UTC), identifier="m-latest-utc")
        with tempfile.TemporaryDirectory() as directory:
            root, report_dir = Path(directory), Path(directory) / "site/reports/send-message-benchmark"
            write_inputs(report_dir, [third, first, second])
            with mock.patch.object(REPORT, "ROOT", ROOT):
                outputs = REPORT.rebuild(report_dir, root / "site/reports", invoke_index=False)
                before = {path.name: path.read_bytes() for path in outputs}
                REPORT.rebuild(report_dir, root / "site/reports", invoke_index=False)
                after = {path.name: path.read_bytes() for path in outputs}
            self.assertEqual(before, after)
            self.assertTrue((report_dir / "phase-ao2.html").is_file())
            self.assertTrue((report_dir / "index.html").is_file())
            self.assertTrue((root / "site/reports/send-message-benchmark.json").is_file())
            phase = (report_dir / "phase-ao2.html").read_text(encoding="utf-8")
            index = (report_dir / "index.html").read_text(encoding="utf-8")
            panel = (report_dir / f"{first.campaign_id}.xhtml").read_text(encoding="utf-8")
        self.assertIn("Aug 23, 2026", REPORT.time_view(first.started_at)["text"])
        self.assertIn("Aug 23, 2026", REPORT.time_view(second.started_at)["text"])
        self.assertGreater(first.started_at, second.started_at)
        self.assertLess(phase.index(third.campaign_id), phase.index(first.campaign_id))
        self.assertLess(phase.index(first.campaign_id), phase.index(second.campaign_id))
        self.assertNotIn("<script src=", phase)
        for page in (panel, phase, index):
            self.assertEqual(page.count("<script"), 1)
            self.assertNotIn('src="http', page)
            self.assertNotIn('href="http', page)
        self.assertIn("TCP + TLS", phase)
        self.assertIn('data-target="tcp-tls"', phase)

    def test_incomplete_panel_xml_and_chart_exclusion(self) -> None:
        incomplete = campaign(datetime(2026, 8, 24, 7, tzinfo=UTC), identifier="20260824T070000Z-rand-m5", status="INCOMPLETE")
        passing = campaign(datetime(2026, 8, 24, 8, tzinfo=UTC), identifier="20260824T080000Z-rand-m5")
        with tempfile.TemporaryDirectory() as directory:
            root, report_dir = Path(directory), Path(directory) / "site/reports/send-message-benchmark"
            write_inputs(report_dir, [incomplete, passing])
            with mock.patch.object(REPORT, "ROOT", ROOT):
                REPORT.rebuild(report_dir, root / "site/reports", invoke_index=False)
            panel = report_dir / f"{incomplete.campaign_id}.xhtml"
            ET.parse(panel)
            panel_text = panel.read_text(encoding="utf-8")
            phase = (report_dir / "phase-ao2.html").read_text(encoding="utf-8")
        self.assertIn("Incomplete campaign:", panel_text)
        self.assertLess(panel_text.index("Incomplete campaign:"), panel_text.index("<script"))
        self.assertEqual(phase.count('class="candle PASS"'), 4)
        self.assertNotIn('class="candle INCOMPLETE"', phase)

    def test_chart_geometry_has_all_candles_series_fail_outline_and_baselines(self) -> None:
        one = campaign(datetime(2026, 8, 24, 7, tzinfo=UTC), identifier="20260824T070000Z-rand-m5")
        two = campaign(datetime(2026, 8, 24, 8, tzinfo=UTC), identifier="20260824T080000Z-rand-m4", host="rand-m4", status="FAIL")
        with tempfile.TemporaryDirectory() as directory:
            report_dir = Path(directory)
            write_inputs(report_dir, [one, two])
            charts = REPORT.candlestick_series(
                REPORT.TARGET_ORDER,
                REPORT.empty_historical_record(),
                [one, two],
                REPORT.load_baselines(report_dir / REPORT.BASELINES_FILENAME),
            )
        self.assertEqual(len(charts["tcp"]["candles"]), 2)
        self.assertEqual(len(charts["tcp"]["series"]), 2)
        self.assertEqual(len(charts["tcp"]["baseline_lines"]), 2)
        self.assertEqual(charts["tcp"]["candles"][1]["status"], "FAIL")

    def test_historical_display_uses_current_ratchet_not_frozen_ingest_floor(self) -> None:
        older = result(
            "tcp", datetime(2026, 8, 24, 7, tzinfo=UTC),
            identifier="20260824T070000Z-rand-m5", floor=16_000,
        )
        later = result(
            "tcp", datetime(2026, 8, 24, 8, tzinfo=UTC),
            identifier="20260824T080000Z-rand-m5", floor=18_000, p50=20_000,
        )
        historical = HistoricalRecord(
            schema_version=1,
            generated_from_commit="a" * 40,
            campaigns=(),
            ratchet=(
                RatchetPoint(
                    host_label="rand-m5", target="tcp", effective_from=older.generated_at,
                    p50_floor=18_000, source_campaign_id=older.campaign_id,
                ),
                RatchetPoint(
                    host_label="rand-m5", target="tcp", effective_from=later.generated_at,
                    p50_floor=20_000, source_campaign_id=later.campaign_id,
                ),
            ),
            unattributed=(),
        )
        self.assertEqual(older.status, "PASS")
        self.assertEqual(REPORT.current_historical_display_status(older, historical), "FAIL")

    def test_preview_copies_the_newest_panel_without_opening_wyvern(self) -> None:
        item = campaign(datetime(2026, 8, 24, 7, tzinfo=UTC), identifier="20260824T070000Z-rand-m5")
        with tempfile.TemporaryDirectory() as directory:
            root, report_dir = Path(directory), Path(directory) / "site/reports/send-message-benchmark"
            write_inputs(report_dir, [item])
            with mock.patch.object(REPORT, "ROOT", ROOT):
                REPORT.rebuild(report_dir, root / "site/reports", invoke_index=False)
                preview = REPORT.preview_latest(report_dir, root / "preview", open_viewer=False)
            self.assertEqual(preview.name, "latest.html")
            self.assertEqual(preview.read_text(), (report_dir / f"{item.campaign_id}.xhtml").read_text())

    def test_no_retired_aggregate_template_or_render_path_remains(self) -> None:
        self.assertFalse((ROOT / "templates/benchmark-report/benchmark-report.xhtml.j2").exists())
        self.assertFalse((ROOT / "templates/benchmark-report/benchmark-report.html.j2").exists())
        self.assertNotIn("--input", SCRIPT.read_text(encoding="utf-8"))
        self.assertNotIn("send-message-benchmark.html", SCRIPT.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
