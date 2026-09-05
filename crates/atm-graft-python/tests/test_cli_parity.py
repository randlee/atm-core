"""End-to-end parity checks for the Rust binding and the ``atm`` CLI.

Local runs stay opt-in: set ``ATM_CLI_PARITY_FIXTURE`` to a JSON fixture
pointing at an operator-owned disposable daemon. CI sets
``ATM_CLI_PARITY_CI=1``; in that mode this module creates an isolated roster,
starts the checked-out CLI's paired daemon, and tears it down after the test.

KNOWN ISOLATION GAP (readiness-review B5, tracked as a follow-up, not fixed
here): ``_GeneratedParityFixture._environment`` below sets ``HOME``,
``ATM_HOME``, ``ATM_LOG_DIR``, and ``TMPDIR`` to a disposable per-run
directory, but ``atm-core::home::current_host_runtime_scope`` (the daemon's
own runtime-scope resolver) intentionally ignores all of those and always
resolves ``owner.lock``, the endpoint, and the durable state root from the
real OS user record (``getpwuid``/equivalent) -- that is deliberate host-scope
behavior for the real daemon, not a bug, and this test file must not change
it. The practical effect is that the "disposable" daemon this fixture starts
actually owns the operator's real ``~/.atm/daemon`` and writes the real
``~/.atm/db``: this class is therefore host-exclusive today. It reliably
passes only on a host with no other ``atm-daemon`` already running (e.g. a
fresh CI VM); running it on a workstation with a live personal daemon will
fail on the ``owner.lock`` acquisition, not because of a defect in this test.
Until a real runtime-scope override lands for tests, treat this suite as
serialized/host-exclusive and do not run it concurrently with any other
``atm-daemon`` instance on the same machine.
"""

from __future__ import annotations

import json
import os
import queue
import threading
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

# Bounded wait for the daemon readiness handshake and for every CLI
# subprocess invocation. Every blocking call in this module must be bounded
# by one of these deadlines rather than able to hang indefinitely: a stalled
# daemon or CLI process fails the test with a clear message instead of
# wedging the whole suite.
_DAEMON_READY_TIMEOUT_SECONDS = 30
_CLI_SUBPROCESS_TIMEOUT_SECONDS = 30


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
        self._stderr_lines: list[str] = []
        self._stderr_thread: threading.Thread | None = None
        self._stdout_thread: threading.Thread | None = None

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
            timeout=_CLI_SUBPROCESS_TIMEOUT_SECONDS,
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

        # Drain stderr continuously on its own thread from the moment the
        # process starts. Without this, a chatty daemon can fill the stderr
        # pipe buffer (commonly 64 KiB) and block on write() forever while
        # this fixture is only reading stdout -- a classic two-process
        # deadlock, not a timing race, and no readiness deadline below can
        # rescue it once that happens.
        assert self.process.stderr is not None
        stderr_pipe = self.process.stderr

        def _drain_stderr() -> None:
            for line in iter(stderr_pipe.readline, ""):
                self._stderr_lines.append(line)

        self._stderr_thread = threading.Thread(target=_drain_stderr, daemon=True)
        self._stderr_thread.start()

        # Pump stdout lines onto a queue on its own thread so the readiness
        # wait below can use `Queue.get(timeout=...)`, a single call with a
        # real bounded deadline, instead of a plain `readline()` that can
        # block past the deadline if the daemon never writes another line.
        assert self.process.stdout is not None
        stdout_pipe = self.process.stdout
        stdout_lines: queue.Queue[str | None] = queue.Queue()

        def _pump_stdout() -> None:
            for line in iter(stdout_pipe.readline, ""):
                stdout_lines.put(line)
            stdout_lines.put(None)

        self._stdout_thread = threading.Thread(target=_pump_stdout, daemon=True)
        self._stdout_thread.start()

        deadline = time.monotonic() + _DAEMON_READY_TIMEOUT_SECONDS
        ready = False
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or self.process.poll() is not None:
                break
            try:
                line = stdout_lines.get(timeout=remaining)
            except queue.Empty:
                break
            if line is None:
                break
            if line.strip() == "ATM_DAEMON_READY":
                ready = True
                break
        if not ready:
            self.stop()
            stderr_text = "".join(self._stderr_lines).strip()
            raise RuntimeError(
                "parity daemon did not become ready within "
                f"{_DAEMON_READY_TIMEOUT_SECONDS}s: {stderr_text}"
            )

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
        if self.process is not None and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        # Bounded joins: once the process has exited, both reader threads
        # observe EOF on their pipe and return on their own; a timeout here
        # only guards against this method itself ever hanging.
        if self._stdout_thread is not None:
            self._stdout_thread.join(timeout=5)
        if self._stderr_thread is not None:
            self._stderr_thread.join(timeout=5)

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
            timeout=_CLI_SUBPROCESS_TIMEOUT_SECONDS,
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
