"""Unit tests for the checked-in AI.38 Hermes steer smoke runner."""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
import unittest


ROOT = Path(__file__).resolve().parents[2]
RUNNER = ROOT / "scripts" / "phase-ai" / "run-hermes-steer-smoke.py"


class HermesSteerSmokeTests(unittest.TestCase):
    def test_fixture_emits_two_safe_boundary_evidence_rows(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(RUNNER), "--fixture"],
            check=True,
            text=True,
            capture_output=True,
            cwd=ROOT,
        )
        rows = json.loads(completed.stdout)
        self.assertEqual({row["wake_kind"] for row in rows}, {"live_nudge", "recovery_summary"})
        for row in rows:
            self.assertTrue(row["steer_accepted"])
            self.assertFalse(row["normal_message_handler_called"])
            self.assertFalse(row["current_task_interrupted"])
            self.assertFalse(row["mailbox_mutated_by_wake"])

    def test_fixture_flag_is_required(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(RUNNER)],
            text=True,
            capture_output=True,
            cwd=ROOT,
        )
        self.assertNotEqual(completed.returncode, 0)
        self.assertIn("--fixture is required", completed.stderr)


if __name__ == "__main__":
    unittest.main()
