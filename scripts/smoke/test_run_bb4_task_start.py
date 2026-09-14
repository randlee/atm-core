"""Unit checks for the BB.4 live task-start runner."""

from __future__ import annotations

import json
import unittest
from unittest import mock

from scripts.smoke import run_bb4_task_start as RUNNER


class Bb4TaskStartRunnerTests(unittest.TestCase):
    def test_passive_assignee_rejects_live_agent_member(self) -> None:
        payload = {
            "result": {
                "agents": [
                    {"name": RUNNER.ASSIGNEE, "pane_id": "w1:p2"},
                ]
            }
        }
        result = mock.Mock(returncode=0, stdout=json.dumps(payload), stderr="")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "has a live agent pane"):
                RUNNER.require_passive_assignee("fixture")

    def test_passive_assignee_accepts_unrelated_live_agent(self) -> None:
        payload = {
            "result": {"agents": [{"name": "tester", "pane_id": "w1:p3"}]}
        }
        result = mock.Mock(returncode=0, stdout=json.dumps(payload), stderr="")
        with mock.patch.object(RUNNER.subprocess, "run", return_value=result):
            RUNNER.require_passive_assignee("fixture")


if __name__ == "__main__":
    unittest.main()
