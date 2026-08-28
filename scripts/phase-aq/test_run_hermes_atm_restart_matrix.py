from __future__ import annotations

import importlib.util
import os
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

    def test_daemon_launch_uses_plaintext_test_wire_security(self) -> None:
        # The runner's daemon runs from a disposable tempfile home with no
        # provisioned peer HTTPS interface or certificate identity, so it
        # must launch in `plaintext-test` mode, never `mutual-tls` (which
        # requires exactly that provisioning and made the matrix unrunnable
        # on a clean CI host).
        module = load_module()
        argv = module.daemon_launch_argv(Path("/fake/atm-daemon"))
        self.assertEqual(argv, [str(Path("/fake/atm-daemon")), "--peer-wire-security", "plaintext-test"])
        self.assertNotIn("mutual-tls", argv)

    def test_crash_recovery_bound_is_sub_tick(self) -> None:
        # AC2 (sprint-AQ1-9) requires sub-tick recovery: strictly inside one
        # GRAFT_LEASE_REFRESH_INTERVAL tick, not a padded multiple of it.
        module = load_module()
        self.assertEqual(module.LEASE_REFRESH_INTERVAL_SECONDS, 1.0)
        self.assertEqual(module.CRASH_RECOVERY_LIMIT_SECONDS, module.LEASE_REFRESH_INTERVAL_SECONDS)

    def test_scoped_process_environment_restores_prior_values_and_absence(self) -> None:
        module = load_module()
        sentinel_key = "AQ19_RESTART_MATRIX_TEST_PRESENT"
        absent_key = "AQ19_RESTART_MATRIX_TEST_ABSENT"
        os.environ[sentinel_key] = "before"
        os.environ.pop(absent_key, None)
        try:
            with module.scoped_process_environment({sentinel_key: "during", absent_key: "during"}):
                self.assertEqual(os.environ[sentinel_key], "during")
                self.assertEqual(os.environ[absent_key], "during")
            self.assertEqual(os.environ[sentinel_key], "before")
            self.assertNotIn(absent_key, os.environ)
        finally:
            os.environ.pop(sentinel_key, None)
            os.environ.pop(absent_key, None)

    def test_scoped_process_environment_restores_on_exception(self) -> None:
        module = load_module()
        absent_key = "AQ19_RESTART_MATRIX_TEST_EXC_ABSENT"
        os.environ.pop(absent_key, None)
        with self.assertRaises(RuntimeError):
            with module.scoped_process_environment({absent_key: "during"}):
                self.assertEqual(os.environ[absent_key], "during")
                raise RuntimeError("boom")
        self.assertNotIn(absent_key, os.environ)


if __name__ == "__main__":
    unittest.main()
