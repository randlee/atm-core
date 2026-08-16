from __future__ import annotations

from pathlib import Path
import sys
import tomllib
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

    def test_ci_runs_the_bridge_boundary_suite_for_each_supported_python(self) -> None:
        workflow = (ROOT / ".github" / "workflows" / "ci.yml").read_text(encoding="utf-8")

        self.assertIn("Run Hermes graft steer boundary tests", workflow)
        self.assertIn("python .just/run_hermes_graft_bridge_tests.py", workflow)
        self.assertIn("Hermes ATM wheel (Python", workflow)
        for version in ("3.11", "3.12", "3.13", "3.14"):
            self.assertIn(f'"{version}"', workflow)

    def test_hermes_atm_declares_the_supported_python_release_range(self) -> None:
        manifest = ROOT / "crates" / "hermes-atm" / "pyproject.toml"
        metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))

        self.assertEqual(metadata["project"]["requires-python"], ">=3.11,<3.15")


if __name__ == "__main__":
    unittest.main()
