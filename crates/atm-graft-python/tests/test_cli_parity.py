"""End-to-end parity checks for the Rust binding and the ``atm`` CLI.

Set ``ATM_CLI_PARITY_FIXTURE`` to a JSON fixture to run this test.  The
fixture is deliberately external: it supplies a disposable daemon, roster,
and identities without embedding machine-specific paths or credentials in
the repository.  A normal package test run skips the live check when no
fixture is supplied.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
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


@unittest.skipUnless(
    os.environ.get("ATM_CLI_PARITY_FIXTURE"),
    "set ATM_CLI_PARITY_FIXTURE to run the disposable-daemon parity matrix",
)
class CliParityTests(unittest.TestCase):
    """Compare native tool JSON with CLI JSON on one disposable daemon."""

    @classmethod
    def setUpClass(cls) -> None:
        fixture_path = Path(os.environ["ATM_CLI_PARITY_FIXTURE"])
        cls.fixture = json.loads(fixture_path.read_text(encoding="utf-8"))
        cls.environment = os.environ.copy()
        cls.environment.update(cls.fixture.get("environment", {}))
        cls.atm = cls.fixture.get("atm", "atm")
        cls.identity = cls.fixture["identity"]
        cls.team = cls.fixture["team"]
        cls.chat_id = cls.fixture.get("chat_id", "parity")

        # Import only for the opted-in live matrix.  This keeps the ordinary
        # source-only Python test suite independent of an installed wheel.
        from hermes_atm import native_tools

        cls.native_tools = native_tools.AtmNativeTools(
            identity=cls.identity,
            team=cls.team,
            chat_id=cls.chat_id,
        )

    @classmethod
    def _cli(cls, *arguments: str) -> dict[str, object]:
        completed = subprocess.run(  # noqa: S603 - fixture supplies an argv, never a shell
            [cls.atm, *arguments, "--json"],
            check=True,
            capture_output=True,
            text=True,
            env=cls.environment,
        )
        return json.loads(completed.stdout)

    @classmethod
    def _native(cls, method: str, arguments: dict[str, object]) -> dict[str, object]:
        return json.loads(getattr(cls.native_tools, method)(arguments))["result"]

    def _assert_parity(
        self,
        cli_arguments: tuple[str, ...],
        native_method: str,
        native_arguments: dict[str, object],
    ) -> None:
        self.assertEqual(
            _without_observability(self._cli(*cli_arguments)),
            _without_observability(self._native(native_method, native_arguments)),
        )

    def test_list_read_send_and_ack_match_cli(self) -> None:
        fixture = self.fixture
        self._assert_parity(
            ("list",),
            "atm_list",
            {"selection": fixture.get("selection", "actionable")},
        )
        self._assert_parity(
            ("read", "--history"),
            "atm_read",
            {"selection": "all"},
        )

        recipient = fixture["recipient"]
        body = fixture.get("body", "native-tool parity")
        self._assert_parity(
            ("send", recipient, body),
            "atm_send",
            {"to": recipient, "body": body},
        )

        ack_message_id = fixture.get("ack_message_id")
        if ack_message_id is not None:
            reply = fixture.get("reply", "acknowledged")
            self._assert_parity(
                ("ack", ack_message_id, reply),
                "atm_ack",
                {"message_id": ack_message_id, "reply": reply},
            )


if __name__ == "__main__":
    unittest.main()
