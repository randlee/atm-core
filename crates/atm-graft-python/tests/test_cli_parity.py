"""End-to-end parity checks for the Rust binding and the ``atm`` CLI.

Local runs stay opt-in: set ``ATM_CLI_PARITY_FIXTURE`` to a JSON fixture
pointing at an operator-owned disposable daemon. CI sets
``ATM_CLI_PARITY_CI=1``; in that mode this module creates an isolated roster,
starts the checked-out CLI's paired daemon, and tears it down after the test.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest


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


def _without_run_specific_values(value: object, key: str = "") -> object:
    """Normalize IDs and timestamps created by the two twin operations."""

    if isinstance(value, dict):
        return {
            name: _without_run_specific_values(item, name)
            for name, item in value.items()
        }
    if isinstance(value, list):
        return [_without_run_specific_values(item, key) for item in value]
    if key in {
        "message_id",
        "reply_message_id",
        "selected_message_id",
        "timestamp",
        "pending_ack_at",
        "acknowledged_at",
        "expires_at",
    }:
        return "<run-specific>"
    return value


class _GeneratedParityFixture:
    """Own the real CLI/daemon pair used by the mandatory CI parity test."""

    team = "aw5-cli-parity"
    receiver = "aw5-parity-receiver"
    sender = "aw5-parity-sender"
    chat_id = "parity"

    def __init__(self) -> None:
        self._temporary = tempfile.TemporaryDirectory(prefix="atm-cli-parity-")
        self.root = Path(self._temporary.name)
        self.process: subprocess.Popen[str] | None = None

    def _environment(self) -> dict[str, str]:
        home = self.root / "home"
        atm_home = self.root / "atm-home"
        log_dir = self.root / "logs"
        temp_dir = self.root / "tmp"
        for directory in (home, atm_home, log_dir, temp_dir):
            directory.mkdir(parents=True, exist_ok=True)
        return {
            **os.environ,
            "HOME": str(home),
            "ATM_HOME": str(atm_home),
            "ATM_CONFIG_HOME": str(atm_home),
            "ATM_TEAM": self.team,
            "ATM_CHAT_ID": self.chat_id,
            "ATM_LOG_DIR": str(log_dir),
            "TMPDIR": str(temp_dir),
            "TMP": str(temp_dir),
            "TEMP": str(temp_dir),
        }

    @staticmethod
    def _binary(environment_name: str, default: Path) -> Path:
        value = os.environ.get(environment_name)
        candidate = Path(value).expanduser().resolve() if value else default.resolve()
        if os.name == "nt" and not candidate.is_file():
            candidate = candidate.with_suffix(".exe")
        return candidate

    def _cli(
        self,
        atm: Path,
        environment: dict[str, str],
        arguments: list[str],
        identity: str,
    ) -> dict[str, object]:
        completed = subprocess.run(
            [str(atm), *arguments, "--json"],
            cwd=Path(__file__).resolve().parents[3],
            env={**environment, "ATM_IDENTITY": identity},
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=30,
            check=False,
        )
        if completed.returncode != 0:
            detail = completed.stderr.strip() or completed.stdout.strip()
            raise RuntimeError(f"CLI command failed: {completed.args}: {detail}")
        return json.loads(completed.stdout)

    def start(self) -> dict[str, object]:
        repo = Path(__file__).resolve().parents[3]
        environment = self._environment()
        atm = self._binary("ATM_PARITY_ATM", repo / "target" / "debug" / "atm")
        daemon = self._binary(
            "ATM_PARITY_DAEMON", repo / "target" / "debug" / "atm-daemon"
        )
        if not atm.is_file() or not daemon.is_file():
            raise RuntimeError(
                "CI parity fixture requires target/debug/atm and target/debug/atm-daemon"
            )

        # Roster setup is deliberately performed by the real CLI before the
        # daemon starts; the test never edits mailbox or roster files itself.
        member_home = self.root / "member-home"
        member_home.mkdir()
        for member in (self.sender, self.receiver):
            self._cli(
                atm,
                environment,
                [
                    "teams",
                    "add-member",
                    self.team,
                    member,
                    "--home-dir",
                    str(member_home),
                ],
                self.sender,
            )

        self.process = subprocess.Popen(
            [str(daemon), "--peer-wire-security", "plaintext-test"],
            cwd=repo,
            env={**environment, "ATM_DAEMON_READY_STDOUT": "1"},
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        deadline = time.monotonic() + 30
        ready = False
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                break
            line = self.process.stdout.readline() if self.process.stdout else ""
            if line.strip() == "ATM_DAEMON_READY":
                ready = True
                break
        if not ready:
            stderr = self.process.stderr.read().strip() if self.process.stderr else ""
            self.stop()
            raise RuntimeError(f"parity daemon did not become ready: {stderr}")

        return {
            "atm": str(atm),
            "environment": environment,
            "identity": self.receiver,
            "second_identity": self.sender,
            "team": self.team,
            "chat_id": self.chat_id,
            "recipient": f"{self.sender}@{self.team}",
        }

    def stop(self) -> None:
        if self.process is None or self.process.poll() is not None:
            return
        self.process.terminate()
        try:
            self.process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)

    def close(self) -> None:
        self.stop()
        self._temporary.cleanup()


def _parity_enabled() -> bool:
    return bool(os.environ.get("ATM_CLI_PARITY_FIXTURE")) or os.environ.get(
        "ATM_CLI_PARITY_CI"
    ) == "1"


@unittest.skipUnless(
    _parity_enabled(),
    "set ATM_CLI_PARITY_FIXTURE locally or use ATM_CLI_PARITY_CI in CI",
)
class CliParityTests(unittest.TestCase):
    """Compare native tool JSON with CLI JSON on one disposable daemon."""

    @classmethod
    def setUpClass(cls) -> None:
        fixture_path = os.environ.get("ATM_CLI_PARITY_FIXTURE")
        cls._generated: _GeneratedParityFixture | None = None
        if fixture_path:
            cls.fixture = json.loads(Path(fixture_path).read_text(encoding="utf-8"))
            cls.environment = os.environ.copy()
            cls.environment.update(cls.fixture.get("environment", {}))
        else:
            cls._generated = _GeneratedParityFixture()
            cls.fixture = cls._generated.start()
            cls.environment = cls.fixture["environment"]

        cls.atm = cls.fixture.get("atm", "atm")
        cls.identity = cls.fixture["identity"]
        cls.second_identity = cls.fixture.get("second_identity", cls.identity)
        cls.team = cls.fixture["team"]
        cls.chat_id = cls.fixture.get("chat_id", "parity")

        from hermes_atm import native_tools

        cls.native_tools = native_tools.AtmNativeTools(
            identity=cls.identity,
            team=cls.team,
            chat_id=cls.chat_id,
        )
        cls.second_native_tools = native_tools.AtmNativeTools(
            identity=cls.second_identity,
            team=cls.team,
            chat_id=cls.chat_id,
        )

    @classmethod
    def tearDownClass(cls) -> None:
        if cls._generated is not None:
            cls._generated.close()

    @classmethod
    def _cli(cls, *arguments: str, identity: str | None = None) -> dict[str, object]:
        environment = {
            **cls.environment,
            "ATM_IDENTITY": identity or cls.identity,
            "ATM_TEAM": cls.team,
            "ATM_CHAT_ID": cls.chat_id,
        }
        completed = subprocess.run(
            [cls.atm, *arguments, "--json"],
            check=True,
            capture_output=True,
            text=True,
            env=environment,
        )
        return json.loads(completed.stdout)

    @classmethod
    def _native(
        cls, tools: object, method: str, arguments: dict[str, object]
    ) -> dict[str, object]:
        return json.loads(getattr(tools, method)(arguments))["result"]

    def _assert_parity(
        self,
        cli_value: dict[str, object],
        native_value: dict[str, object],
        *,
        normalize_run_specific: bool = False,
    ) -> None:
        cli_value = _without_observability(cli_value)
        native_value = _without_observability(native_value)
        if normalize_run_specific:
            cli_value = _without_run_specific_values(cli_value)
            native_value = _without_run_specific_values(native_value)
        self.assertEqual(cli_value, native_value)

    def test_list_read_send_and_ack_match_cli(self) -> None:
        fixture = self.fixture
        self._assert_parity(
            self._cli("list", identity=self.identity),
            self._native(
                self.native_tools,
                "atm_list",
                {"selection": fixture.get("selection", "actionable")},
            ),
        )
        self._assert_parity(
            self._cli("read", "--history", identity=self.identity),
            self._native(self.native_tools, "atm_read", {"selection": "all"}),
        )

        recipient = fixture.get("recipient", f"{self.second_identity}@{self.team}")
        body = fixture.get("body", "native-tool parity")
        self._assert_parity(
            self._cli("send", recipient, body, identity=self.identity),
            self._native(
                self.native_tools,
                "atm_send",
                {"to": recipient, "body": body},
            ),
            normalize_run_specific=True,
        )

        # Send two independent requires_ack twins from the second identity.
        # The receiver acks one natively and the other through the CLI so the
        # two acknowledgement result envelopes can be compared fairly.
        native_send = self._native(
            self.second_native_tools,
            "atm_send",
            {
                "to": f"{self.identity}@{self.team}",
                "body": "native ack twin",
                "requires_ack": True,
            },
        )
        cli_send = self._cli(
            "send",
            f"{self.identity}@{self.team}",
            "cli ack twin",
            "--requires-ack",
            identity=self.second_identity,
        )
        native_ack = self._native(
            self.native_tools,
            "atm_ack",
            {"message_id": native_send["message_id"], "reply": "parity reply"},
        )
        cli_ack = self._cli(
            "ack",
            cli_send["message_id"],
            "parity reply",
            identity=self.identity,
        )
        self._assert_parity(cli_ack, native_ack, normalize_run_specific=True)

        native_pending = self._native(
            self.native_tools, "atm_list", {"selection": "pending_ack"}
        )
        cli_pending = self._cli(
            "list", "--pending-ack", identity=self.identity
        )
        for pending in (native_pending, cli_pending):
            pending_ids = {row["message_id"] for row in pending["rows"]}
            self.assertNotIn(native_send["message_id"], pending_ids)
            self.assertNotIn(cli_send["message_id"], pending_ids)


if __name__ == "__main__":
    unittest.main()
