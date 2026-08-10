from __future__ import annotations

import importlib.util
from pathlib import Path
import sys
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / ".just" / "run_smoke.py"


def load_module():
    spec = importlib.util.spec_from_file_location("run_smoke", SCRIPT)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class SmokeDispatchTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = load_module()

    def test_special_feature_uses_its_canonical_runner(self) -> None:
        completed = mock.Mock(returncode=0)
        with (
            mock.patch.object(sys, "argv", [str(SCRIPT), "graft-hermes", "--report"]),
            mock.patch.object(self.module.subprocess, "run", return_value=completed) as run,
        ):
            self.assertEqual(self.module.main(), 0)

        run.assert_called_once_with(
            [
                sys.executable,
                str(self.module.SPECIAL_RUNNERS["graft-hermes"]),
                "--report",
            ],
            cwd=ROOT,
            check=False,
        )

    def test_normal_feature_uses_the_shared_feature_runner(self) -> None:
        completed = mock.Mock(returncode=7)
        with (
            mock.patch.object(sys, "argv", [str(SCRIPT), "localhost"]),
            mock.patch.object(self.module.subprocess, "run", return_value=completed) as run,
        ):
            self.assertEqual(self.module.main(), 7)

        run.assert_called_once_with(
            [
                sys.executable,
                str(ROOT / "scripts" / "smoke" / "run_feature_smoke.py"),
                "localhost",
            ],
            cwd=ROOT,
            check=False,
        )


if __name__ == "__main__":
    unittest.main()
