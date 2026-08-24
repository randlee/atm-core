from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import shutil
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))


SCRIPT = ROOT / "scripts/smoke/migrate_benchmark_history.py"
spec = importlib.util.spec_from_file_location("migrate_benchmark_history", SCRIPT)
assert spec and spec.loader
MIGRATE = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = MIGRATE
spec.loader.exec_module(MIGRATE)


class MigrateBenchmarkHistoryTests(unittest.TestCase):
    def fixture_dir(self) -> Path:
        return ROOT / "scripts/smoke/fixtures/benchmark-legacy"

    def migrated_report_dir(self) -> Path:
        return ROOT / "site/reports/send-message-benchmark"

    def test_real_shape_fixtures_preserve_source_bytes_and_metrics(self) -> None:
        names = (
            "20260801-063351.969883-mac-arm64-01-uds-f1.json",
            "20260801-200412.808565-windows-x64-01-tcp-f1.json",
            "20260809-193021.750417-local-tcp-f1.json",
            "20260820-220233.418717-local-tcp-f1.json",
        )
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp) / "reports"
            reports.mkdir()
            for name in names:
                shutil.copy2(self.fixture_dir() / name, reports / name)
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            before = {name: (reports / name).read_bytes() for name in names}
            record, audit = MIGRATE.migrated_record(reports, "0" * 40)
            after = {name: (reports / name).read_bytes() for name in names}
        self.assertEqual(after, before)
        self.assertEqual(len(record.campaigns), 4)
        self.assertEqual(audit["source_count"], 4)
        source = json.loads(before[names[0]])
        migrated = next(item for item in audit["mappings"] if item["source_file"] == names[0])
        self.assertEqual(migrated["generated_at"], source["generated_at"])
        self.assertEqual(
            migrated["metrics"]["admissions_per_second"]["p50"],
            source["metrics"]["admissions_per_second"]["p50"],
        )

    def test_check_mode_names_a_corrupted_legacy_fixture(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            source["metrics"]["requested_count"] = -1
            (reports / "broken.json").write_text(json.dumps(source), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            with self.assertRaisesRegex(MIGRATE.MigrationError, "broken.json"):
                MIGRATE.main(["--reports-dir", str(reports), "--check"])

    def test_invalid_legacy_source_names_the_offending_file(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = reports / "broken.json"
            source.write_text(json.dumps({"schema_version": 3, "artifact_kind": "send_message_benchmark_summary"}), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            with self.assertRaisesRegex(MIGRATE.MigrationError, "broken.json"):
                MIGRATE.migrated_record(reports, "0" * 40)

    def test_valid_but_unclassifiable_source_is_retained_as_unattributed(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            source["host_label"] = "unknown-host"
            source["host_os"] = None
            (reports / "orphan.json").write_text(json.dumps(source), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            record, audit = MIGRATE.migrated_record(reports, "0" * 40)
        self.assertEqual(record.campaigns, ())
        self.assertEqual(record.unattributed[0].source_file, "orphan.json")
        self.assertIn("missing host OS", record.unattributed[0].reason)
        self.assertEqual(audit["unattributed_count"], 1)

    def test_v1_summary_shape_keeps_recorded_timestamp_and_p50(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            source["schema_version"] = 1
            (reports / "v1-result.json").write_text(json.dumps(source), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            record, audit = MIGRATE.migrated_record(reports, "0" * 40)
        migrated = record.campaigns[0].results[0].result
        self.assertEqual(migrated.generated_at.isoformat().replace("+00:00", "Z"), source["generated_at"])
        self.assertEqual(migrated.metrics.admissions_per_second.p50, source["metrics"]["admissions_per_second"]["p50"])
        self.assertEqual(audit["mappings"][0]["source_file"], "v1-result.json")

    def test_post_cleanup_tree_is_a_valid_noop(self) -> None:
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            (reports / "historical-record.json").write_text(
                (self.migrated_report_dir() / "historical-record.json").read_text(encoding="utf-8"),
                encoding="utf-8",
            )
            (reports / "baselines.json").write_text(
                (self.migrated_report_dir() / "baselines.json").read_text(encoding="utf-8"),
                encoding="utf-8",
            )
            self.assertEqual(MIGRATE.main(["--reports-dir", str(reports), "--check"]), 0)

    def test_record_setting_run_is_classified_against_the_prior_ratchet_floor(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            earlier = json.loads(json.dumps(source))
            earlier["generated_at"] = "2026-08-01T06:00:00Z"
            later = json.loads(json.dumps(source))
            later["generated_at"] = "2026-08-01T06:01:00Z"
            earlier_rate = earlier["metrics"]["admissions_per_second"]["p50"]
            for percentile in later["metrics"]["admissions_per_second"]:
                later["metrics"]["admissions_per_second"][percentile] *= 2
            (reports / "earlier.json").write_text(json.dumps(earlier), encoding="utf-8")
            (reports / "later.json").write_text(json.dumps(later), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            record, _audit = MIGRATE.migrated_record(reports, "0" * 40)
        entries = [entry for campaign in record.campaigns for entry in campaign.results]
        later_entry = next(entry for entry in entries if entry.result.generated_at.isoformat().endswith("06:01:00+00:00"))
        self.assertEqual(later_entry.displayed_status, "PASS")
        self.assertEqual(record.ratchet[0].p50_floor, earlier_rate)
        self.assertLess(record.ratchet[0].p50_floor, later_entry.result.metrics.admissions_per_second.p50)

    def test_original_status_and_d8_display_can_diverge(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            # Preserve a legacy source FAIL while its D8 comparison against
            # the empty ratchet should display PASS.
            source["passed"] = False
            (reports / "original-fail.json").write_text(json.dumps(source), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            record, _audit = MIGRATE.migrated_record(reports, "0" * 40)
        entry = record.campaigns[0].results[0]
        self.assertEqual(entry.result.status, "FAIL")
        self.assertEqual(entry.displayed_status, "PASS")
        self.assertGreater(entry.result.baseline.p50_floor, entry.result.metrics.admissions_per_second.p50)

    def test_updated_baselines_adds_only_a_passing_windows_seed(self) -> None:
        source_name = "20260801-063351.969883-mac-arm64-01-uds-f1.json"
        with tempfile.TemporaryDirectory() as temp:
            reports = Path(temp)
            source = json.loads((self.fixture_dir() / source_name).read_text(encoding="utf-8"))
            source.update({
                "host_label": "windows-x64-02",
                "host_os": "windows",
                "transport": "tcp",
                "peer_wire_security": "plaintext-test",
                "benchmark_target": "tcp",
            })
            (reports / "windows-passing.json").write_text(json.dumps(source), encoding="utf-8")
            (reports / "baselines.json").write_text(
                json.dumps({"schema_version": 1, "revision": 1, "entries": []}), encoding="utf-8"
            )
            record, _audit = MIGRATE.migrated_record(reports, "0" * 40)
            updated = MIGRATE.updated_baselines(reports, record)
        seed = updated.entry_for("windows-x64-02", "tcp")
        self.assertEqual(updated.revision, 2)
        self.assertEqual(seed.p50_floor, source["metrics"]["admissions_per_second"]["p50"])
        self.assertEqual(seed.effective_from.isoformat().replace("+00:00", "Z"), source["generated_at"])
        self.assertEqual(seed.approved_by, "historical migration seed; pending quality review")


if __name__ == "__main__":
    unittest.main()
