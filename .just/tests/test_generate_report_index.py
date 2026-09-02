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


def write_smoke_envelope(root: Path, platform: str, host: str, run: str) -> str:
    reports = root / "site/reports"
    report_dir = reports / "smoke" / platform / host / run
    report_dir.mkdir(parents=True)
    (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
    report_html = report_dir.relative_to(reports).joinpath("index.html").as_posix()
    (report_dir / "smoke.envelope.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "report_type": "smoke",
                "generated_at": "2026-08-08T04:00:00Z",
                "host_label": host,
                "report_html": report_html,
                "status": "PASS",
            }
        ),
        encoding="utf-8",
    )
    return report_html


class GenerateReportIndexTests(unittest.TestCase):
    def test_empty_input_has_every_report_group(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            (root / "site/reports").mkdir(parents=True)
            index = build_index(root / "site/reports")
            self.assertIn("<h2>Benchmark</h2>", index)
            self.assertIn("<h2>Fuzz</h2>", index)
            self.assertIn("<h2>Smoke</h2>", index)
            self.assertEqual(index.count("No reports available."), 3)

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

    def test_accepts_a_benchmark_index_inside_its_evidence_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            reports = Path(tempdir) / "site/reports"
            evidence = reports / "send-message-benchmark"
            evidence.mkdir(parents=True)
            (evidence / "index.html").write_text("<html>benchmark index</html>\n", encoding="utf-8")
            (reports / "send-message-benchmark.json").write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-24T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark/index.html",
            }), encoding="utf-8")
            index = build_index(reports)
        self.assertIn('href="send-message-benchmark/index.html"', index)

    def test_canonical_benchmark_index_supersedes_but_does_not_delete_run_sidecars(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            reports = Path(tempdir) / "site/reports"
            evidence = reports / "send-message-benchmark"
            evidence.mkdir(parents=True)
            (evidence / "index.html").write_text("<html>index</html>\n", encoding="utf-8")
            (reports / "send-message-benchmark.json").write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-24T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark/index.html",
            }), encoding="utf-8")
            old = evidence / "old.envelope.json"
            old.write_text(json.dumps({
                "schema_version": 1, "report_type": "benchmark",
                "generated_at": "2026-08-01T00:00:00Z", "host_label": "rand-m5",
                "report_html": "send-message-benchmark.html",
            }), encoding="utf-8")
            index = build_index(reports)
            self.assertTrue(old.exists())
        self.assertIn("send-message-benchmark/index.html", index)
        self.assertNotIn("send-message-benchmark.html", index)

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

    def test_ignores_nested_report_type_artifact_without_envelope_fields(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            evidence = reports / "read-query-benchmark"
            evidence.mkdir(parents=True)
            (reports / "read-query-benchmark.html").write_text(
                "<html>benchmark</html>\n", encoding="utf-8"
            )
            (reports / "read-query-benchmark.json").write_text(
                json.dumps({
                    "schema_version": 1,
                    "report_type": "benchmark",
                    "generated_at": "2026-07-01T00:00:00Z",
                    "host_label": "mac-arm64",
                    "report_html": "read-query-benchmark.html",
                }),
                encoding="utf-8",
            )
            (evidence / "family.json").write_text(
                json.dumps({"schema_version": 1, "report_type": "read-query-benchmark", "families": []}),
                encoding="utf-8",
            )
            index = build_index(reports)
            self.assertIn('href="read-query-benchmark.html"', index)

    def test_discovers_every_nested_smoke_run_as_a_browsable_master_link(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            first = write_smoke_envelope(
                root,
                "windows",
                "FastPC4",
                "20260808T032327Z-pid22208-localhost",
            )
            second = write_smoke_envelope(
                root,
                "macos",
                "rand-m4",
                "20260808T040000Z-pid19288-local-ip",
            )

            index = build_index(root / "site/reports")

            self.assertIn("<h2>Smoke</h2>", index)
            self.assertIn(f'href="{first}"', index)
            self.assertIn(f'href="{second}"', index)
            self.assertIn(first.removesuffix("/index.html"), index)
            self.assertIn(second.removesuffix("/index.html"), index)

    def test_rejects_smoke_envelope_outside_its_run_directory(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            report_dir = reports / "smoke" / "windows" / "FastPC4" / "run"
            report_dir.mkdir(parents=True)
            (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
            (reports / "wrong-place.envelope.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "report_type": "smoke",
                        "generated_at": "2026-08-08T04:00:00Z",
                        "host_label": "FastPC4",
                        "report_html": "smoke/windows/FastPC4/run/index.html",
                        "status": "PASS",
                    }
                ),
                encoding="utf-8",
            )

            with self.assertRaisesRegex(ReportIndexError, "stored beside"):
                build_index(reports)

    def test_discovers_legacy_smoke_result_without_a_new_envelope(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            reports = root / "site/reports"
            report_dir = reports / "smoke" / "windows" / "FastPC4" / "20260808T032327Z-pid1-localhost"
            report_dir.mkdir(parents=True)
            (report_dir / "index.html").write_text("<html>smoke</html>\n", encoding="utf-8")
            (report_dir / "localhost.json").write_text(
                json.dumps(
                    {
                        "feature": "localhost",
                        "platform": "windows",
                        "host": "FastPC4",
                        "run_id": "20260808T032327Z",
                        "status": "FAIL",
                        "cases": [],
                    }
                ),
                encoding="utf-8",
            )

            index = build_index(reports)

            self.assertIn('href="smoke/windows/FastPC4/20260808T032327Z-pid1-localhost/index.html"', index)
            self.assertIn("FAIL", index)

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
