"""CI-only CLI/native parity fixture with an explicitly owned daemon."""

from __future__ import annotations

import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.smoke.daemon_lifecycle import require_clean_host_daemon_state


_CLI_TIMEOUT_SECONDS = 60
_DAEMON_READY_TIMEOUT_SECONDS = 30
_DAEMON_STOP_TIMEOUT_SECONDS = 10


def _without_observability(value: object) -> object:
    if isinstance(value, dict):
        return {
            key: _without_observability(item)
            for key, item in value.items()
            if key != "observability"
        }
    if isinstance(value, list):
        return [_without_observability(item) for item in value]
    return value


class _CiParityFixture:
    """A clean-runner fixture that owns one replacement daemon child."""

    team = "aw5-cli-parity"
    sender = "aw5-parity-sender"
    receiver = "aw5-parity-receiver"

    def __init__(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="atm-cli-parity-")
        self.root = Path(self.temporary.name)
        self.environment = {
            **os.environ,
            "ATM_TEAM": self.team,
            "ATM_CHAT_ID": "parity",
            "ATM_LOG_DIR": str(self.root / "logs"),
        }
        self.daemon: subprocess.Popen[str] | None = None

    @staticmethod
    def binary() -> Path:
        candidate = Path(os.environ.get("ATM_PARITY_ATM", ROOT / "target" / "debug" / "atm"))
        if os.name == "nt" and not candidate.is_file():
            candidate = candidate.with_suffix(".exe")
        if not candidate.is_file():
            raise RuntimeError("CI parity fixture requires target/debug/atm")
        return candidate.resolve()

    @staticmethod
    def daemon_binary() -> Path:
        candidate = Path(
            os.environ.get("ATM_PARITY_DAEMON", _CiParityFixture.binary().with_name("atm-daemon"))
        )
        if os.name == "nt" and not candidate.is_file():
            candidate = candidate.with_suffix(".exe")
        if not candidate.is_file():
            raise RuntimeError("CI parity fixture requires target/debug/atm-daemon")
        return candidate.resolve()

    def cli(self, *arguments: str, identity: str) -> dict[str, object]:
        completed = subprocess.run(
            [str(self.binary()), *arguments, "--json"],
            cwd=ROOT,
            env={**self.environment, "ATM_IDENTITY": identity},
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=_CLI_TIMEOUT_SECONDS,
            check=False,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or completed.stdout.strip()
            print(f"CLI parity failure log directory: {self.environment['ATM_LOG_DIR']}")
            print(f"CLI parity stderr:\n{completed.stderr}")
            print(f"CLI parity stdout:\n{completed.stdout}")
            raise RuntimeError(f"CLI command failed: {completed.args}: {detail}")
        return json.loads(completed.stdout)

    def start_daemon(self) -> None:
        require_clean_host_daemon_state(smoke_label="CLI/native parity fixture")
        self.daemon = subprocess.Popen(
            [str(self.daemon_binary()), "--peer-wire-security", "plaintext-test"],
            cwd=ROOT,
            env={**self.environment, "ATM_DAEMON_READY_STDOUT": "1"},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        if not _daemon_ready(self.daemon, _DAEMON_READY_TIMEOUT_SECONDS):
            self.stop_daemon()
            raise RuntimeError("CI parity daemon did not publish ATM_DAEMON_READY")

    def stop_daemon(self) -> None:
        if self.daemon is None or self.daemon.poll() is not None:
            return
        self.daemon.terminate()
        try:
            self.daemon.wait(timeout=_DAEMON_STOP_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired:
            self.daemon.kill()
            self.daemon.wait(timeout=_DAEMON_STOP_TIMEOUT_SECONDS)

    def start(self) -> None:
        member_home = self.root / "member-home"
        member_home.mkdir()
        for member in (self.sender, self.receiver):
            self.cli("teams", "add-member", self.team, member, "--home-dir", str(member_home), identity=self.sender)
        try:
            self.start_daemon()
            self.cli("list", identity=self.receiver)
        except BaseException:
            self.stop_daemon()
            raise

    def close(self) -> None:
        try:
            self.stop_daemon()
        finally:
            self.temporary.cleanup()


def _daemon_ready(process: subprocess.Popen[str], timeout: float) -> bool:
    """Wait for the replacement daemon marker without a blocking pipe read."""
    lines: "queue.Queue[str | None]" = queue.Queue()

    def pump_stdout() -> None:
        if process.stdout is None:
            lines.put(None)
            return
        try:
            for line in iter(process.stdout.readline, ""):
                lines.put(line)
        finally:
            lines.put(None)

    threading.Thread(target=pump_stdout, daemon=True).start()
    deadline = time.monotonic() + timeout
    while (remaining := deadline - time.monotonic()) > 0:
        try:
            line = lines.get(timeout=remaining)
        except queue.Empty:
            return False
        if line is None:
            return False
        if line.strip() == "ATM_DAEMON_READY":
            return True
    return False


@unittest.skipUnless(
    os.environ.get("ATM_CLI_PARITY_CI") == "1",
    "CLI/native parity is an explicitly invoked CI-only fixture",
)
class CliParityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fixture = _CiParityFixture()
        cls.fixture.start()
        cls.addClassCleanup(cls.fixture.close)
        cls.prior_environment = os.environ.copy()
        os.environ.update(cls.fixture.environment)
        from hermes_atm import native_tools

        cls.native_receiver = native_tools.AtmNativeTools(
            identity=cls.fixture.receiver,
            team=cls.fixture.team,
            chat_id="parity",
        )
        cls.native_sender = native_tools.AtmNativeTools(
            identity=cls.fixture.sender,
            team=cls.fixture.team,
            chat_id="parity",
        )

    @classmethod
    def tearDownClass(cls) -> None:
        os.environ.clear()
        os.environ.update(cls.prior_environment)

    @staticmethod
    def native(tools: object, method: str, arguments: dict[str, object]) -> dict[str, object]:
        return json.loads(getattr(tools, method)(arguments))["result"]

    def assert_parity(self, cli: dict[str, object], native: dict[str, object]) -> None:
        self.assertEqual(_without_observability(cli), _without_observability(native))

    def test_list_and_read_match(self) -> None:
        self.assert_parity(
            self.fixture.cli("list", identity=self.fixture.receiver),
            self.native(self.native_receiver, "atm_list", {"selection": "actionable"}),
        )
        self.assert_parity(
            self.fixture.cli("read", "--history", identity=self.fixture.receiver),
            self.native(self.native_receiver, "atm_read", {"selection": "all"}),
        )

    def test_send_and_ack_match(self) -> None:
        recipient = f"{self.fixture.receiver}@{self.fixture.team}"
        cli_send = self.fixture.cli("send", recipient, "cli parity", "--requires-ack", identity=self.fixture.sender)
        native_send = self.native(
            self.native_sender,
            "atm_send",
            {"to": recipient, "body": "native parity", "requires_ack": True},
        )
        cli_ack = self.fixture.cli("ack", cli_send["message_id"], "parity reply", identity=self.fixture.receiver)
        native_ack = self.native(
            self.native_receiver,
            "atm_ack",
            {"message_id": native_send["message_id"], "reply": "parity reply"},
        )
        self.assertEqual(cli_ack["message_id"], cli_send["message_id"])
        self.assertEqual(native_ack["message_id"], native_send["message_id"])
        self.assertEqual(cli_ack["action"], native_ack["action"])
        self.assertEqual(cli_ack["team"], native_ack["team"])
        self.assertEqual(cli_ack["agent"], native_ack["agent"])
        self.assertEqual(cli_ack["reply_text"], native_ack["reply_text"])
        self.assertEqual(
            cli_ack["reply_disposition"]["kind"],
            native_ack["reply_disposition"]["kind"],
        )
        self.assertEqual(
            cli_ack["reply_disposition"]["reply_target"],
            native_ack["reply_disposition"]["reply_target"],
        )


if __name__ == "__main__":
    unittest.main()
