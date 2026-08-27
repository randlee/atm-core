from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace


SCRIPT = Path(__file__).with_name("run_hermes_atm_restart_matrix.py")


def load_module():
    spec = importlib.util.spec_from_file_location("run_hermes_atm_restart_matrix", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class HermesAtmRestartMatrixTests(unittest.TestCase):
    def test_evidence_names_all_three_rows_and_records_pending_m5_as_host_specific(self) -> None:
        module = load_module()
        with tempfile.TemporaryDirectory() as temporary:
            args = SimpleNamespace(host="m5", evidence_dir=Path(temporary))
            records = [
                {"id": "daemon-restart-live-receiver", "status": "pass", "delivery_latency_ms": 12.0},
                {"id": "receiver-restart-live-daemon", "status": "pass", "delivery_latency_ms": 8.0},
                {"id": "receiver-crash-within-window", "status": "pass", "delivery_latency_ms": 5.0},
            ]
            json_path, markdown_path = module.write_evidence(args, records)

            self.assertEqual(json_path.name, "restart-matrix-m5.json")
            self.assertEqual(markdown_path.name, "restart-matrix-m5.md")
            markdown = markdown_path.read_text(encoding="utf-8")
            self.assertIn("daemon-restart-live-receiver", markdown)
            self.assertIn("receiver-restart-live-daemon", markdown)
            self.assertIn("receiver-crash-within-window", markdown)
            self.assertIn("no remote result is inferred", markdown)

    def test_crash_recovery_bound_is_one_and_a_half_refresh_ticks(self) -> None:
        module = load_module()
        self.assertEqual(module.LEASE_REFRESH_INTERVAL_SECONDS, 1.0)
        self.assertEqual(module.CRASH_RECOVERY_LIMIT_SECONDS, 1.5)


if __name__ == "__main__":
    unittest.main()
