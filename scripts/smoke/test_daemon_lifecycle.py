"""Regression tests for exact, owner-scoped local daemon process detection."""
from __future__ import annotations

import subprocess
import unittest
from unittest import mock

from scripts.smoke import daemon_lifecycle


class DaemonLifecycleTests(unittest.TestCase):
    def test_unix_detection_matches_only_our_daemon_executable_not_shell_arguments(self) -> None:
        listing = "\n".join((
            "501 42 /opt/homebrew/bin/atm-daemon",
            "501 43 zsh -c python runner.py --daemon-link /opt/homebrew/bin/atm-daemon --yes",
            "502 44 /opt/homebrew/bin/atm-daemon",
        ))
        completed = subprocess.CompletedProcess(["ps"], 0, stdout=listing, stderr="")
        with mock.patch.object(daemon_lifecycle.os, "name", "posix"), mock.patch.object(
            daemon_lifecycle.subprocess, "run", return_value=completed
        ) as run, mock.patch.object(
            daemon_lifecycle.os, "getuid", return_value=501, create=True,
        ):
            self.assertEqual(daemon_lifecycle.count_atm_daemon_processes(), [42])
        self.assertEqual(run.call_args.args[0], ["ps", "-axo", "uid=,pid=,command="])

    def test_unix_detection_ignores_another_users_ambient_daemon(self) -> None:
        completed = subprocess.CompletedProcess(
            ["ps"], 0, stdout="501 42 /opt/homebrew/bin/atm-daemon", stderr="",
        )
        with mock.patch.object(daemon_lifecycle.os, "name", "posix"), mock.patch.object(
            daemon_lifecycle.subprocess, "run", return_value=completed
        ), mock.patch.object(daemon_lifecycle.os, "getuid", return_value=502, create=True):
            self.assertEqual(daemon_lifecycle.count_atm_daemon_processes(), [])


if __name__ == "__main__":
    unittest.main()
