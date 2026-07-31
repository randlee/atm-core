"""Unit tests for the checked-in AI.38 Hermes steer smoke runner."""

from __future__ import annotations

import json
import importlib.util
import io
from pathlib import Path
import sys
import unittest
from contextlib import redirect_stdout
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts" / "phase-ai" / "run-hermes-steer-smoke.py"


class HermesSteerSmokeTests(unittest.TestCase):
    def test_fixture_emits_two_safe_boundary_evidence_rows(self) -> None:
        runner = load_runner_module()

        async def fixture_evidence():
            return [
                evidence_row("live_nudge"),
                evidence_row("recovery_summary"),
            ]

        with (
            mock.patch.object(runner, "parse_args", return_value=type("Args", (), {"fixture": True})()),
            mock.patch.object(runner, "load_fixture_evidence", return_value=fixture_evidence),
            io.StringIO() as output,
            redirect_stdout(output),
        ):
            runner.main()
            rows = json.loads(output.getvalue())
        self.assertEqual({row["wake_kind"] for row in rows}, {"live_nudge", "recovery_summary"})
        for row in rows:
            self.assertTrue(row["steer_accepted"])
            self.assertFalse(row["normal_message_handler_called"])
            self.assertFalse(row["current_task_interrupted"])
            self.assertFalse(row["mailbox_mutated_by_wake"])

    def test_fixture_flag_is_required(self) -> None:
        runner = load_runner_module()
        with mock.patch.object(runner, "parse_args", return_value=type("Args", (), {"fixture": False})()):
            with self.assertRaisesRegex(SystemExit, "--fixture is required"):
                runner.main()


def load_runner_module():
    spec = importlib.util.spec_from_file_location("run_hermes_steer_smoke", RUNNER)
    if spec is None or spec.loader is None:
        raise AssertionError("could not load Hermes steer smoke runner")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def evidence_row(wake_kind: str) -> dict[str, object]:
    return {
        "profile": "agent@team",
        "chat_id": "session",
        "wake_kind": wake_kind,
        "steer_accepted": True,
        "normal_message_handler_called": False,
        "current_task_interrupted": False,
        "mailbox_mutated_by_wake": False,
    }


if __name__ == "__main__":
    unittest.main()
