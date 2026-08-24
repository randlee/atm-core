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
        return ROOT / "site/reports/send-message-benchmark"

    def test_real_shape_fixtures_preserve_source_bytes_and_metrics(self) -> None:
        names = (
            "20260801-063351.969883-mac-arm64-01-uds-f1.json",
            "20260801-200412.808565-windows-x64-01-tcp-f1.json",
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
        self.assertEqual(len(record.campaigns), 2)
        self.assertEqual(audit["source_count"], 2)
        source = json.loads(before[names[0]])
        migrated = next(item for item in audit["mappings"] if item["source_file"] == names[0])
        self.assertEqual(migrated["generated_at"], source["generated_at"])
        self.assertEqual(
            migrated["metrics"]["admissions_per_second"]["p50"],
            source["metrics"]["admissions_per_second"]["p50"],
        )

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


if __name__ == "__main__":
    unittest.main()
