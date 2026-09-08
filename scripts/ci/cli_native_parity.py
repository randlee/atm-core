"""CI-only CLI/native parity fixture using the ordinary daemon launch gate."""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


_CLI_TIMEOUT_SECONDS = 30


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
    """A clean-runner fixture; it never launches ``atm-daemon`` directly."""

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

    @staticmethod
    def binary() -> Path:
        repo = Path(__file__).resolve().parents[2]
        candidate = Path(os.environ.get("ATM_PARITY_ATM", repo / "target" / "debug" / "atm"))
        if os.name == "nt" and not candidate.is_file():
            candidate = candidate.with_suffix(".exe")
        if not candidate.is_file():
            raise RuntimeError("CI parity fixture requires target/debug/atm")
        return candidate.resolve()

    def cli(self, *arguments: str, identity: str) -> dict[str, object]:
        completed = subprocess.run(
            [str(self.binary()), *arguments, "--json"],
            cwd=Path(__file__).resolve().parents[2],
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

    def start(self) -> None:
        member_home = self.root / "member-home"
        member_home.mkdir()
        for member in (self.sender, self.receiver):
            self.cli("teams", "add-member", self.team, member, "--home-dir", str(member_home), identity=self.sender)
        # This normal CLI request invokes DaemonSupervisor's production gate:
        # connect first, then spawn only when no daemon answers. A CI runner
        # is dedicated to this fixture, and a pre-existing daemon makes the
        # daemon startup guard abort rather than creating a second runtime.
        self.cli("list", identity=self.receiver)

    def close(self) -> None:
        self.temporary.cleanup()


@unittest.skipUnless(
    os.environ.get("ATM_CLI_PARITY_CI") == "1",
    "CLI/native parity is an explicitly invoked CI-only fixture",
)
class CliParityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.fixture = _CiParityFixture()
        cls.fixture.start()
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
        try:
            cls.fixture.close()
        finally:
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
        self.assertEqual(cli_ack["acknowledged"], native_ack["acknowledged"])


if __name__ == "__main__":
    unittest.main()
