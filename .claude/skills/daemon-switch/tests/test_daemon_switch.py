"""Regression tests for the daemon-switch singleton cleanup rules."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import socket
import tempfile
import unittest


SCRIPT = Path(__file__).parents[1] / "scripts" / "daemon-switch.py"
SPEC = importlib.util.spec_from_file_location("daemon_switch", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
DAEMON_SWITCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DAEMON_SWITCH)


class StaleSocketCleanupTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.home = Path(self.temporary.name)
        self.socket_path = self.home / ".atm" / "daemon" / "atm-daemon.sock"
        self.socket_path.parent.mkdir(parents=True)
        self.original_path = DAEMON_SWITCH.Path
        self.original_owners = DAEMON_SWITCH.macos_socket_owner_pids

        class TestPath:
            @staticmethod
            def home() -> Path:
                return self.home

        DAEMON_SWITCH.Path = TestPath
        DAEMON_SWITCH.macos_socket_owner_pids = lambda: []

    def tearDown(self) -> None:
        DAEMON_SWITCH.Path = self.original_path
        DAEMON_SWITCH.macos_socket_owner_pids = self.original_owners
        self.temporary.cleanup()

    def test_removes_unowned_unix_socket(self) -> None:
        listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        listener.bind(str(self.socket_path))
        listener.close()

        self.assertTrue(DAEMON_SWITCH.remove_verified_stale_macos_socket(None))

        self.assertFalse(self.socket_path.exists())

    def test_refuses_regular_file_at_daemon_socket_path(self) -> None:
        self.socket_path.write_text("not a socket", encoding="utf-8")

        with self.assertRaisesRegex(DAEMON_SWITCH.SwitchError, "non-socket"):
            DAEMON_SWITCH.remove_verified_stale_macos_socket(None)

        self.assertTrue(self.socket_path.is_file())


if __name__ == "__main__":
    unittest.main()
