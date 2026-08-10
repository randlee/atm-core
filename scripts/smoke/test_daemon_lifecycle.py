"""Regression tests for exact local daemon process detection."""
from __future__ import annotations

import subprocess
import unittest
from unittest import mock

from scripts.smoke import daemon_lifecycle


class DaemonLifecycleTests(unittest.TestCase):
    def test_unix_detection_matches_only_the_executable_not_shell_arguments(self) -> None:
        listing = "\n".join((
            "42 /opt/homebrew/bin/atm-daemon",
            "43 zsh -c python runner.py --daemon-link /opt/homebrew/bin/atm-daemon --yes",
        ))
        completed = subprocess.CompletedProcess(["ps"], 0, stdout=listing, stderr="")
        with mock.patch.object(daemon_lifecycle.os, "name", "posix"), mock.patch.object(
            daemon_lifecycle.subprocess, "run", return_value=completed
        ):
            self.assertEqual(daemon_lifecycle.count_atm_daemon_processes(), [42])


if __name__ == "__main__":
    unittest.main()
