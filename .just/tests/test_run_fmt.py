from __future__ import annotations

from pathlib import Path
import sys
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

import run_fmt


class RunFmtTests(unittest.TestCase):
    def test_nonzero_child_exit_propagates(self) -> None:
        with mock.patch("run_fmt.os.chdir"), mock.patch(
            "run_fmt.subprocess.run", return_value=mock.Mock(returncode=37)
        ) as run:
            self.assertEqual(run_fmt.main(["run_fmt.py", "check"]), 37)

        run.assert_called_once_with(["cargo", "fmt", "--all", "--check"], check=False)
