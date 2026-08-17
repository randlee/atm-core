"""Focused contract tests for the Hermes graft smoke backend."""

from __future__ import annotations

import argparse
import importlib.util
import os
from pathlib import Path
import unittest


SCRIPT = Path(__file__).with_name("run-hermes-graft-smoke.py")
SPEC = importlib.util.spec_from_file_location("run_hermes_graft_smoke", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class HermesGraftSmokeTests(unittest.TestCase):
    def test_acknowledgement_uses_native_cli_with_receiver_context(self) -> None:
        args = argparse.Namespace(
            agent="receiver",
            team="hermes",
            chat_id="8991600178",
            workspace_root="/tmp/hermes-workspace",
        )
        previous_binary = os.environ.get("ATM_BIN")
        os.environ["ATM_BIN"] = "/tmp/atm"
        try:
            command, environment = MODULE.acknowledgement_command(
                args,
                "01KZRP9J2X0Q13CJXYT6F6HBRD",
                "confirmed",
            )
        finally:
            if previous_binary is None:
                del os.environ["ATM_BIN"]
            else:
                os.environ["ATM_BIN"] = previous_binary

        self.assertEqual(
            command,
            [
                "/tmp/atm",
                "ack",
                "--team",
                "hermes",
                "01KZRP9J2X0Q13CJXYT6F6HBRD",
                "confirmed",
            ],
        )
        self.assertEqual(environment["ATM_IDENTITY"], "receiver")
        self.assertEqual(environment["ATM_TEAM"], "hermes")
        self.assertEqual(environment["ATM_CHAT_ID"], "8991600178")


if __name__ == "__main__":
    unittest.main()
