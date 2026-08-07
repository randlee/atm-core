from __future__ import annotations

import unittest
from unittest import mock
import sys
from pathlib import Path

SMOKE_ROOT = Path(__file__).resolve().parent
if str(SMOKE_ROOT) not in sys.path:
    sys.path.insert(0, str(SMOKE_ROOT))

import run_graft_same_host as RUNNER


class IsolatedGraftSmokeTests(unittest.TestCase):
    def test_preflight_refuses_to_begin_with_an_ambient_daemon(self) -> None:
        with (
            mock.patch.object(RUNNER, "require_clean_host_daemon_state") as require_clean,
            mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[42]),
        ):
            with self.assertRaisesRegex(RuntimeError, "ambient atm-daemon"):
                RUNNER.isolated_daemon_baseline()

        require_clean.assert_called_once_with(smoke_label="graft same-host smoke")

    def test_preflight_captures_an_empty_owned_process_baseline(self) -> None:
        with (
            mock.patch.object(RUNNER, "require_clean_host_daemon_state") as require_clean,
            mock.patch.object(RUNNER, "count_atm_daemon_processes", return_value=[]),
        ):
            self.assertEqual(RUNNER.isolated_daemon_baseline(), [])

        require_clean.assert_called_once_with(smoke_label="graft same-host smoke")


if __name__ == "__main__":
    unittest.main()
