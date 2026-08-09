from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts/smoke/benchmark_report.py"
spec = importlib.util.spec_from_file_location("benchmark_report", SCRIPT)
assert spec and spec.loader
REPORT = importlib.util.module_from_spec(spec)
spec.loader.exec_module(REPORT)


class BenchmarkReportTests(unittest.TestCase):
    def fixture(self, name: str) -> Path:
        return ROOT / ".just/fixtures/benchmark" / name

    def test_migrates_v1_and_strips_private_fields(self) -> None:
        result = REPORT.load_result(self.fixture("legacy-v1.json"))
        self.assertEqual(result["migration"], {"from_schema_version": 1})
        self.assertEqual(result["schema_version"], 3)
        self.assertEqual(result["metrics"]["accepted_count"], 1_000)
        encoded = json.dumps(REPORT.load_result(self.fixture("success-uds-f1.json")))
        self.assertNotIn("/Users/", encoded)
        self.assertNotIn("peer_host", encoded)

    def test_transport_and_profile_are_preserved(self) -> None:
        uds = REPORT.load_result(self.fixture("success-uds-f1.json"))
        tcp = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        self.assertEqual((uds["transport"], uds["frames_per_connection"]), ("uds", 1))
        self.assertEqual((tcp["transport"], tcp["frames_per_connection"]), ("tcp", 8))

    def test_direct_sqlite_measurement_is_retained_and_rendered(self) -> None:
        payload = json.loads(self.fixture("success-uds-f1.json").read_text(encoding="utf-8"))
        payload["direct_sqlite_message_write"] = {
            "kind": "async_storage_admission",
            "requested_count": 10_000,
            "accepted_count": 10_000,
            "worker_count": 64,
            "elapsed_seconds": 0.2,
            "admissions_per_second": 50_000.0,
        }
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "result.json"
            source.write_text(json.dumps(payload), encoding="utf-8")
            result = REPORT.load_result(source)
            with mock.patch.object(REPORT, "ROOT", ROOT):
                panel = REPORT.render_run(result, "sqlite-probe", Path(directory))
                aggregate = REPORT.render_aggregate([result], Path(directory))
            panel_text = panel.read_text(encoding="utf-8")
            aggregate_text = aggregate.read_text(encoding="utf-8")
        self.assertEqual(result["direct_sqlite_message_write"]["accepted_count"], 10_000)
        self.assertIn("50000.00", panel_text)
        self.assertIn("Direct SQLite msg/s", aggregate_text)

    def test_source_revision_is_retained_only_when_it_is_a_git_revision(self) -> None:
        payload = json.loads(self.fixture("success-uds-f1.json").read_text(encoding="utf-8"))
        payload["source_revision"] = "a" * 40
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "result.json"
            source.write_text(json.dumps(payload), encoding="utf-8")
            self.assertEqual(REPORT.load_result(source)["source_revision"], "a" * 40)
            payload["source_revision"] = "not-a-revision"
            source.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(REPORT.BenchmarkReportError, "source_revision"):
                REPORT.load_result(source)

    def test_failed_run_is_retained(self) -> None:
        result = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        self.assertFalse(result["passed"])
        self.assertEqual(result["metrics"]["accepted_count"], 999)
        self.assertEqual(result["failure"], "one admission failed")

    def test_immutable_write_rejects_mutation(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "artifact.json"
            self.assertTrue(REPORT.immutable_write(path, "one\n"))
            self.assertFalse(REPORT.immutable_write(path, "one\n"))
            with self.assertRaisesRegex(REPORT.BenchmarkReportError, "immutable"):
                REPORT.immutable_write(path, "two\n")

    def test_persist_writes_ai46_envelope_sidecar(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            report_dir = Path(directory) / "report"
            with mock.patch.object(REPORT, "REPORT_DIR", report_dir):
                result, artifact_id = REPORT.persist(self.fixture("success-uds-f1.json"))
            artifact = json.loads((report_dir / f"{artifact_id}.json").read_text())
            envelope = json.loads((report_dir / f"{artifact_id}.envelope.json").read_text())
            self.assertEqual(artifact["host_label"], "mac-arm64-01")
            self.assertEqual(set(envelope), {"schema_version", "report_type", "generated_at", "host_label", "report_html"})
            self.assertEqual(envelope["report_type"], "benchmark")

    def test_envelope_for_uses_the_validated_result_identity(self) -> None:
        result = REPORT.load_result(self.fixture("success-uds-f1.json"))
        envelope = json.loads(REPORT.envelope_for(result))
        self.assertEqual(envelope["generated_at"], result["generated_at"])
        self.assertEqual(envelope["host_label"], result["host_label"])
        self.assertEqual(envelope["report_html"], "send-message-benchmark.html")

    def test_aggregate_orders_utc_history_and_separates_transports(self) -> None:
        records = [
            REPORT.load_result(self.fixture("success-uds-f1.json")),
            REPORT.load_result(self.fixture("failed-tcp-f8.json")),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(REPORT, "ROOT", ROOT):
                output = REPORT.render_aggregate(records, root)
            text = output.read_text(encoding="utf-8")
            self.assertIn("mac-arm64-01", text)
            self.assertIn("uds", text)
            self.assertIn("tcp", text)
            self.assertLess(text.index("2026-08-01T01:00:00Z"), text.index("2026-08-01T01:01:00Z"))

    def test_latest_profile_state_supersedes_older_failed_history(self) -> None:
        failed = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        recovered = {**failed, "generated_at": "2026-08-01T02:00:00Z", "passed": True}
        latest = REPORT.latest_profile_results([failed, recovered])
        self.assertEqual(latest, [recovered])
        with tempfile.TemporaryDirectory() as directory:
            output = REPORT.render_aggregate([failed, recovered], Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("Latest profile state: 1 profiles, 1 passed, 0 failed.", text)
        self.assertIn("2 historical runs retained.", text)


if __name__ == "__main__":
    unittest.main()
