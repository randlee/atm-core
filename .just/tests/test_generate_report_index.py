from __future__ import annotations

from pathlib import Path
import json
import sys
import tempfile
import unittest


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from generate_report_index import ReportIndexError
from generate_report_index import build_index
from generate_report_index import write_or_check


def write_envelope(root: Path, name: str, report_type: str, generated_at: str, host: str) -> None:
    reports = root / "site/reports"
    reports.mkdir(parents=True, exist_ok=True)
    (reports / f"{name}.html").write_text(f"<html>{name}</html>\n", encoding="utf-8")
    (reports / name).mkdir(exist_ok=True)
    (reports / name / "evidence.json").write_text("{}\n", encoding="utf-8")
    (reports / f"{name}.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "report_type": report_type,
                "generated_at": generated_at,
                "host_label": host,
                "report_html": f"{name}.html",
            }
        ),
        encoding="utf-8",
    )


class GenerateReportIndexTests(unittest.TestCase):
    def test_empty_input_has_both_groups(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "site/reports").mkdir(parents=True)
            index = build_index(root / "site/reports")
            self.assertIn("<h2>Benchmark</h2>", index)
            self.assertIn("<h2>Fuzz</h2>", index)
            self.assertEqual(index.count("No reports available."), 2)

    def test_aggregates_benchmark_and_orders_entries_newest_first(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            # Two run envelopes intentionally point to the same aggregate HTML.
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "benchmark.html").write_text("<html>benchmark</html>\n", encoding="utf-8")
            (reports / "benchmark").mkdir()
            for name, host, timestamp in (
                ("benchmark", "mac-arm64", "2026-07-01T00:00:00Z"),
                ("benchmark-run-2", "linux-x64", "2026-07-03T00:00:00Z"),
            ):
                source = reports / f"{name}.json"
                source.write_text(
                    json.dumps(
                        {
                            "schema_version": 1,
                            "report_type": "benchmark",
                            "generated_at": timestamp,
                            "host_label": host,
                            "report_html": "benchmark.html",
                        }
                    ),
                    encoding="utf-8",
                )
            write_envelope(root, "campaign", "fuzz", "2026-07-02T00:00:00Z", "mac-arm64")
            index = build_index(reports)
            self.assertEqual(index.count('href="benchmark.html"'), 1)
            self.assertIn("2 runs", index)
            self.assertLess(index.index("benchmark.html"), index.index("campaign.html"))
            self.assertIn("hosts: linux-x64, mac-arm64", index)

    def test_rejects_unsafe_or_incomplete_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "bad.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "benchmark",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "machine name",
                        "report_html": "bad.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "host_label"):
                build_index(reports)

    def test_rejects_missing_html_and_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            (reports / "missing.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "fuzz",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "missing.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "missing report HTML"):
                build_index(reports)

    def test_discovers_nested_run_envelope_without_treating_evidence_as_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            evidence = reports / "campaign"
            evidence.mkdir(parents=True)
            (reports / "campaign.html").write_text("<html>campaign</html>\n", encoding="utf-8")
            (evidence / "run-001.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "fuzz",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "campaign.html",
                    }
                ),
                encoding="utf-8",
            )
            (evidence / "metrics.json").write_text(
                json.dumps({"schema_version": 2, "accepted": 10}), encoding="utf-8"
            )
            index = build_index(reports)
            self.assertIn('href="campaign.html"', index)
            self.assertNotIn("metrics", index)

    def test_check_detects_stale_index(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "site/reports").mkdir(parents=True)
            write_or_check(root, check=False)
            self.assertEqual(write_or_check(root, check=True), 0)
            index_path = root / "site/reports/index.html"
            index_path.write_text(index_path.read_text(encoding="utf-8") + "<!-- stale -->\n", encoding="utf-8")
            with self.assertRaisesRegex(ReportIndexError, "stale"):
                write_or_check(root, check=True)

    def test_rejects_report_symlink_that_escapes_public_root(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir, tempfile.TemporaryDirectory() as outside:
            root = Path(tempdir)
            reports = root / "site/reports"
            reports.mkdir(parents=True)
            outside_path = Path(outside) / "private.html"
            outside_path.write_text("private\n", encoding="utf-8")
            (reports / "leak.html").symlink_to(outside_path)
            (reports / "leak").mkdir()
            (reports / "leak.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "benchmark",
                        "generated_at": "2026-07-01T00:00:00Z",
                        "host_label": "mac-arm64",
                        "report_html": "leak.html",
                    }
                ),
                encoding="utf-8",
            )
            with self.assertRaisesRegex(ReportIndexError, "escapes"):
                build_index(reports)


if __name__ == "__main__":
    unittest.main()
