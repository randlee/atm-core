"""Unit coverage for replacement-runtime preflight in the CLI smoke harness."""
from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest


def load_runner():
    path = Path(__file__).with_name("run_thorough_shared_host.py")
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("run_thorough_shared_host", path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


RUNNER = load_runner()


class ReplacementRuntimePreflightTests(unittest.TestCase):
    def test_accepts_ready_replacement_runtime_with_owner(self):
        self.assertEqual(
            RUNNER.assert_replacement_runtime_status(
                {
                    "runtime_status": {
                        "liveness": "running",
                        "readiness": "ready",
                        "singleton_owner_pid": 42,
                    }
                },
                label="fixture",
            ),
            42,
        )

    def test_rejects_missing_or_nonready_runtime_status(self):
        with self.assertRaisesRegex(RuntimeError, "did not publish"):
            RUNNER.assert_replacement_runtime_status({}, label="fixture")
        with self.assertRaisesRegex(RuntimeError, "not running and ready"):
            RUNNER.assert_replacement_runtime_status(
                {"runtime_status": {"liveness": "running", "readiness": "unavailable"}},
                label="fixture",
            )
        with self.assertRaisesRegex(RuntimeError, "owner PID"):
            RUNNER.assert_replacement_runtime_status(
                {
                    "runtime_status": {
                        "liveness": "running",
                        "readiness": "ready",
                        "singleton_owner_pid": 0,
                    }
                },
                label="fixture",
            )


if __name__ == "__main__":
    unittest.main()
