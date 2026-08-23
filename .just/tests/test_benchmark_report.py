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
        payload["campaign_id"] = "20260822T200000Z-aaaaaaaaaaaa"
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "result.json"
            source.write_text(json.dumps(payload), encoding="utf-8")
            result = REPORT.load_result(source)
            with mock.patch.object(REPORT, "ROOT", ROOT):
                panel = REPORT.render_run(result, "sqlite-probe", Path(directory))
                aggregate = REPORT.render_aggregate([result], Path(directory))
                campaign = REPORT.render_campaign(result["campaign_id"], [result], Path(directory) / "send-message-benchmark")
            panel_text = panel.read_text(encoding="utf-8")
            aggregate_text = aggregate.read_text(encoding="utf-8")
            campaign_text = campaign.read_text(encoding="utf-8")
        self.assertEqual(result["direct_sqlite_message_write"]["accepted_count"], 10_000)
        self.assertIn("50000.00", panel_text)
        self.assertIn("20260822T200000Z-aaaaaaaaaaaa", aggregate_text)
        self.assertIn("Result msg/sec", campaign_text)

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

    def test_hash_bound_historical_import_is_rendered_without_rewriting_raw_artifact(self) -> None:
        payload = json.loads(self.fixture("success-uds-f1.json").read_text(encoding="utf-8"))
        payload.update({
            "benchmark_target": "uds",
            "source_revision": "a" * 40,
            "host_label": "local",
        })
        with tempfile.TemporaryDirectory() as directory:
            report_dir = Path(directory) / "send-message-benchmark"
            report_dir.mkdir()
            artifact = report_dir / "historical.json"
            artifact.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
            manifest = {
                "schema_version": 1,
                "imports": [{
                    "filename": "historical.json",
                    "sha256": REPORT.file_sha256(artifact),
                    "campaign_id": "historical-20260823-aaaaaaaaaaaa",
                    "display_host_label": "m5-atmbench",
                    "provenance_note": "Exact imported source artifact.",
                }],
            }
            (report_dir / REPORT.HISTORICAL_IMPORTS_NAME).write_text(json.dumps(manifest), encoding="utf-8")
            records = REPORT.evidence_records(report_dir)
            self.assertEqual(REPORT.campaign_id(records[0]), "historical-20260823-aaaaaaaaaaaa")
            self.assertEqual(records[0]["_report_display_host_label"], "m5-atmbench")
            with mock.patch.object(REPORT, "ROOT", ROOT):
                output = REPORT.render_aggregate(records, Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("m5-atmbench", text)
        self.assertIn("Exact imported source artifact.", text)

    def test_failed_run_is_retained(self) -> None:
        result = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        self.assertFalse(result["passed"])
        self.assertEqual(result["metrics"]["accepted_count"], 999)
        self.assertEqual(result["failure"], "one admission failed")

    def test_campaign_table_renders_an_incomplete_target_without_metrics(self) -> None:
        failed = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        failed["campaign_id"] = "20260822T200000Z-aaaaaaaaaaaa"
        failed["benchmark_target"] = "tcp"
        failed["metrics"] = None
        rows = REPORT.campaign_target_rows([failed])
        tcp = next(row for row in rows if row["test"] == "tcp")
        self.assertIsNone(tcp["result_msg_per_second"])
        self.assertEqual(tcp["comparison"], "N/A — no empirical baseline artifact")

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

    def test_rebuild_does_not_rewrite_existing_raw_evidence(self) -> None:
        """A rendering rebuild must not normalize or replace immutable raw JSON."""
        payload = json.loads(self.fixture("failed-tcp-f8.json").read_text(encoding="utf-8"))
        payload.update({
            "campaign_id": "20260822T200000Z-aaaaaaaaaaaa",
            "benchmark_target": "tcp-tls",
            "peer_wire_security": "mutual-tls",
        })
        with tempfile.TemporaryDirectory() as directory:
            report_dir = Path(directory) / "send-message-benchmark"
            report_dir.mkdir()
            (report_dir / "legacy-run.json").write_text(json.dumps(payload), encoding="utf-8")
            result = REPORT.load_result(report_dir / "legacy-run.json")
            original = (report_dir / "legacy-run.json").read_text(encoding="utf-8")
            with (
                mock.patch.object(REPORT, "REPORT_DIR", report_dir),
                mock.patch.object(REPORT, "evidence_records", return_value=[result]),
                mock.patch.object(REPORT, "render_run"),
                mock.patch.object(REPORT, "render_campaign"),
                mock.patch.object(REPORT, "render_aggregate"),
                mock.patch.object(REPORT, "regenerate_index"),
            ):
                self.assertEqual(REPORT.process([]), 0)
            self.assertEqual((report_dir / "legacy-run.json").read_text(encoding="utf-8"), original)

    def test_envelope_for_uses_the_validated_result_identity(self) -> None:
        result = REPORT.load_result(self.fixture("success-uds-f1.json"))
        envelope = json.loads(REPORT.envelope_for(result))
        self.assertEqual(envelope["generated_at"], result["generated_at"])
        self.assertEqual(envelope["host_label"], result["host_label"])
        self.assertEqual(envelope["report_html"], "send-message-benchmark.html")

    def test_aggregate_orders_utc_history_and_separates_transports(self) -> None:
        records = [
            {**REPORT.load_result(self.fixture("success-uds-f1.json")), "campaign_id": "20260822T010000Z-aaaaaaaaaaaa"},
            {**REPORT.load_result(self.fixture("failed-tcp-f8.json")), "campaign_id": "20260822T020000Z-bbbbbbbbbbbb"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(REPORT, "ROOT", ROOT):
                output = REPORT.render_aggregate(records, root)
            text = output.read_text(encoding="utf-8")
        self.assertIn("mac-arm64-01", text)
        self.assertIn("20260822T010000Z-aaaaaaaaaaaa", text)
        self.assertIn("20260822T020000Z-bbbbbbbbbbbb", text)

    def test_latest_profile_state_supersedes_older_failed_history(self) -> None:
        failed = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        recovered = {
            **failed, "generated_at": "2026-08-01T02:00:00Z", "passed": True,
            "campaign_id": "20260801T020000Z-aaaaaaaaaaaa",
        }
        latest = REPORT.latest_profile_results([failed, recovered])
        self.assertEqual(latest, [recovered])
        with tempfile.TemporaryDirectory() as directory:
            output = REPORT.render_aggregate([failed, recovered], Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("20260801T020000Z-aaaaaaaaaaaa", text)
        self.assertIn("1 pre-campaign artifacts", text)

    def test_current_campaign_is_complete_only_for_all_six_frames_of_one_candidate(self) -> None:
        base = REPORT.load_result(self.fixture("success-uds-f1.json"))
        revision = "b" * 40
        campaign = [
            {
                **base,
                "generated_at": f"2026-08-01T02:{index:02d}:00Z",
                "transport": "tcp",
                "frames_per_connection": frame,
                "source_revision": revision,
            }
            for index, frame in enumerate(sorted(REPORT.SUPPORTED_FRAMES))
        ]
        self.assertEqual(REPORT.current_campaign_results(campaign), campaign)
        self.assertEqual(REPORT.campaign_status(campaign), "PASS")
        self.assertEqual(REPORT.campaign_status(campaign[:-1]), "INFO")
        self.assertEqual(REPORT.campaign_status([{**campaign[0], "passed": False}]), "FAIL")

    def test_peer_wire_mode_is_rendered_and_separates_campaigns(self) -> None:
        plain = REPORT.load_result(self.fixture("failed-tcp-f8.json"))
        mutual_tls = {
            **plain,
            "generated_at": "2026-08-01T03:00:00Z",
            "peer_wire_security": "mutual-tls",
            "benchmark_target": "tcp-tls",
            "hook_mode": "active",
            "passed": True,
            "campaign_id": "20260801T030000Z-aaaaaaaaaaaa",
        }
        self.assertEqual(REPORT.campaign_groups([plain, mutual_tls]), {
            "20260801T030000Z-aaaaaaaaaaaa": [mutual_tls],
        })
        with tempfile.TemporaryDirectory() as directory:
            output = REPORT.render_aggregate([plain, mutual_tls], Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("20260801T030000Z-aaaaaaaaaaaa", text)
        self.assertIn("1 pre-campaign artifacts", text)

    def test_campaign_xhtml_has_the_required_four_target_table(self) -> None:
        base = REPORT.load_result(self.fixture("success-uds-f1.json"))
        campaign_id = "20260822T200000Z-aaaaaaaaaaaa"
        targets = [
            {**base, "campaign_id": campaign_id, "benchmark_target": "sqlite", "transport": "sqlite", "peer_wire_security": "plaintext-test"},
            {**base, "campaign_id": campaign_id, "benchmark_target": "uds", "transport": "uds", "peer_wire_security": "plaintext-test"},
            {**base, "campaign_id": campaign_id, "benchmark_target": "tcp", "transport": "tcp", "peer_wire_security": "plaintext-test"},
            {**base, "campaign_id": campaign_id, "benchmark_target": "tcp-tls", "transport": "tcp", "peer_wire_security": "mutual-tls"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            output = REPORT.render_campaign(campaign_id, targets, Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("Result msg/sec", text)
        self.assertIn("Historical baseline msg/sec", text)
        self.assertIn("tcp-tls", text)

    def test_campaign_table_refuses_to_mix_source_revisions(self) -> None:
        base = REPORT.load_result(self.fixture("success-uds-f1.json"))
        records = [
            {**base, "benchmark_target": "tcp", "source_revision": "a" * 40},
            {**base, "benchmark_target": "tcp-tls", "source_revision": "b" * 40},
        ]
        with self.assertRaisesRegex(REPORT.BenchmarkReportError, "mix source revisions"):
            REPORT.campaign_target_rows(records)

    def test_bounded_benchmark_failure_code_is_rendered(self) -> None:
        failed = {
            **REPORT.load_result(self.fixture("failed-tcp-f8.json")),
            "benchmark_evidence_failure_code": "missing_compatible_plaintext_baseline",
        }
        with tempfile.TemporaryDirectory() as directory:
            output = REPORT.render_run(failed, "bounded-failure", Path(directory))
            text = output.read_text(encoding="utf-8")
        self.assertIn("Evidence failure code", text)
        self.assertIn("missing_compatible_plaintext_baseline", text)


if __name__ == "__main__":
    unittest.main()
