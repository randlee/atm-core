"""Controlled failure diagnostics for the CI parity daemon fixture."""

from __future__ import annotations

import io
from pathlib import Path
import unittest
from unittest import mock

from scripts.ci import cli_native_parity as parity


class CliParityFixtureTests(unittest.TestCase):
    def test_failed_first_launch_reports_daemon_stderr(self) -> None:
        fixture = parity._CiParityFixture()
        process = mock.Mock(returncode=2, stderr=io.StringIO("peer port claims directory denied"))
        process.poll.return_value = 2
        try:
            with (
                mock.patch.object(parity, "require_clean_host_daemon_state"),
                mock.patch.object(parity._CiParityFixture, "daemon_binary", return_value=Path("/fake/atm-daemon")),
                mock.patch.object(parity.subprocess, "Popen", return_value=process),
                mock.patch.object(parity, "_daemon_ready", return_value=False),
            ):
                with self.assertRaisesRegex(
                    RuntimeError, "exit code 2.*stderr:\\npeer port claims directory denied"
                ):
                    fixture.start_daemon()
        finally:
            fixture.close()


if __name__ == "__main__":
    unittest.main()
