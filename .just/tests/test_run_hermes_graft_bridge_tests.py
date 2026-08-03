from __future__ import annotations

from pathlib import Path
import sys
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
ROOT = JUST_DIR.parent
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from run_hermes_graft_bridge_tests import bridge_test_environment


class HermesGraftBridgeRunnerTests(unittest.TestCase):
    def test_environment_does_not_inject_checked_in_adapter_source(self) -> None:
        with mock.patch.dict("os.environ", {"PATH": "/system/bin"}, clear=True):
            environment = bridge_test_environment(Path("/tmp/venv"), Path("/tmp/venv/bin/python"))

        self.assertEqual(Path(environment["VIRTUAL_ENV"]), Path("/tmp/venv"))
        self.assertNotIn("PYTHONPATH", environment)
        self.assertNotIn("HERMES_SRC", environment)

    def test_ci_test_matrix_runs_the_bridge_boundary_suite(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")

        self.assertIn("Run Hermes graft steer boundary tests", workflow)
        self.assertIn("python .just/run_hermes_graft_bridge_tests.py", workflow)


if __name__ == "__main__":
    unittest.main()
