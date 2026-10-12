from __future__ import annotations

from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock


JUST_DIR = Path(__file__).resolve().parents[1]
if str(JUST_DIR) not in sys.path:
    sys.path.insert(0, str(JUST_DIR))

from lint_cargo_deny import DNS_RETRY_DELAY_SECONDS
from lint_cargo_deny import run_published_deny


class CargoDenyRetryTests(unittest.TestCase):
    @mock.patch("lint_cargo_deny.time.sleep")
    @mock.patch("lint_cargo_deny.subprocess.run")
    def test_retries_only_transient_dns_resolution_failures(
        self, run: mock.Mock, sleep: mock.Mock
    ) -> None:
        dns_failure = subprocess.CompletedProcess(
            args=[], returncode=1, stdout="", stderr="could not resolve hostname"
        )
        success = subprocess.CompletedProcess(args=[], returncode=0, stdout="ok", stderr="")
        run.side_effect = [dns_failure, success]

        completed = run_published_deny(["published-deny"], Path("/tmp/atm"))

        self.assertIs(completed, success)
        self.assertEqual(run.call_count, 2)
        sleep.assert_called_once_with(DNS_RETRY_DELAY_SECONDS)

    @mock.patch("lint_cargo_deny.time.sleep")
    @mock.patch("lint_cargo_deny.subprocess.run")
    def test_does_not_retry_non_dns_failure(self, run: mock.Mock, sleep: mock.Mock) -> None:
        failure = subprocess.CompletedProcess(args=[], returncode=1, stdout="", stderr="policy failed")
        run.return_value = failure

        self.assertIs(run_published_deny(["published-deny"], Path("/tmp/atm")), failure)
        run.assert_called_once()
        sleep.assert_not_called()


if __name__ == "__main__":
    unittest.main()
