"""Unit checks for the BB.6 live prompt-handoff runner."""

from __future__ import annotations

import unittest
from unittest import mock

from scripts.smoke import run_bb6_prompt_handoffs as RUNNER


class Bb6PromptHandoffRunnerTests(unittest.TestCase):
    def test_scenario_inventory_is_exact(self) -> None:
        self.assertEqual(
            RUNNER.SCENARIO_NAMES,
            (
                "task_events_shows_ready_reminders_and_started_for_prompted_task_only",
                "disabled_task_reminder_override_yields_no_handoff_and_doctor_finding",
            ),
        )

    def test_json_value_rejects_failed_and_malformed_results(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "fixture failed"):
            RUNNER.json_value(
                {"exit_code": 1, "stdout": "", "stderr": "fixture failed"},
                "fixture",
            )
        with self.assertRaisesRegex(RuntimeError, "did not return JSON"):
            RUNNER.json_value(
                {"exit_code": 0, "stdout": "not-json", "stderr": ""},
                "fixture",
            )

    def test_blocks_for_separates_task_prompt_lines(self) -> None:
        task = "BB6-one"
        text = (
            f'<atm task="{task}" queued="1" message="one" />\n'
            '<atm task="BB6-two" queued="2" message="two" />\n'
            f'<atm task="{task}" reminder="1" message="one" />\n'
        )
        blocks = RUNNER.blocks_for(text, task)
        self.assertEqual(len(blocks), 2)
        self.assertNotIn("BB6-two", "\n".join(blocks))

    def test_integer_result_rejects_command_failure_and_non_integer(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "count failed"):
            RUNNER.integer_result(
                {"exit_code": 1, "stdout": "", "stderr": "db unavailable"},
                "count",
            )
        with self.assertRaisesRegex(RuntimeError, "non-integer"):
            RUNNER.integer_result(
                {"exit_code": 0, "stdout": "many", "stderr": ""}, "count"
            )

    def test_pane_terminal_lines_classifies_each_prompt(self) -> None:
        task = "BB6-one"
        text = (
            f'<atm task="{task}" queued="1" />\n'
            f'<atm task="{task}" ready></atm>\n'
            f'<atm task="{task}" reminder="2"></atm>\n'
        )
        lines = RUNNER.pane_terminal_lines(text, [task])
        self.assertEqual(
            [(line["kind"], line["attempt"]) for line in lines],
            [("task_queued", 0), ("task_ready", 0), ("task_reminder", 2)],
        )

    def test_terminal_and_stored_identities_compare_exactly(self) -> None:
        terminal = [
            {
                "agent": "tester",
                "task_id": "BB6-one",
                "kind": "task_ready",
                "attempt": 0,
            }
        ]
        stored = [["tester", "task_ready", "BB6-one", 0, "atm:01TEST"]]
        self.assertEqual(
            RUNNER.terminal_identities(terminal), RUNNER.handoff_identities(stored)
        )

    def test_mailbox_terminal_lines_records_surface_and_kind(self) -> None:
        messages = [
            {
                "count": 1,
                "message": {"taskId": "BB6-one", "taskOp": {"op": "start"}},
            },
            {"count": 0},
        ]
        results = [
            {"exit_code": 0, "stdout": __import__("json").dumps(row), "stderr": ""}
            for row in messages
        ]
        with mock.patch.object(RUNNER, "run_cli", side_effect=results):
            lines, commands = RUNNER.mailbox_terminal_lines("fixture", ["BB6-one"])
        self.assertEqual(len(commands), 2)
        self.assertEqual(lines[0]["surface"], "atm_mailbox")
        self.assertEqual(lines[0]["kind"], "task_started")

    def test_handoff_and_event_extractors_reject_wrong_shapes(self) -> None:
        self.assertEqual(RUNNER.handoff_kinds({"handoffs": "bad"}), [])
        self.assertEqual(RUNNER.event_names({"events": "bad"}), [])
        self.assertEqual(
            RUNNER.handoff_kinds({"handoffs": [{"kind": "task_ready"}, "bad"]}),
            ["task_ready"],
        )

    def test_agent_pane_rejects_missing_assignee(self) -> None:
        result = {
            "exit_code": 0,
            "stdout": '{"result":{"agents":[]}}',
            "stderr": "",
        }
        with mock.patch.object(RUNNER, "run_herdr", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "no pane"):
                RUNNER.agent_pane("fixture")

    def test_wait_for_pane_reports_timeout(self) -> None:
        result = {"exit_code": 0, "stdout": "unchanged", "stderr": ""}
        with mock.patch.object(RUNNER, "WAIT_SECONDS", 0), mock.patch.object(
            RUNNER, "pane_text", return_value=result
        ):
            with self.assertRaisesRegex(RuntimeError, "timed out waiting for ready"):
                RUNNER.wait_for_pane("fixture", lambda _text: False, "ready")

    def test_fresh_fixture_refuses_existing_handoffs(self) -> None:
        count = {"exit_code": 0, "stdout": "1\n", "stderr": ""}
        with mock.patch.object(RUNNER, "sqlite_count", return_value=count):
            with self.assertRaisesRegex(RuntimeError, "fresh fixture required"):
                RUNNER.run("fixture", mock.Mock())

    def test_render_html_escapes_case_details_and_shows_counts(self) -> None:
        report = {
            "status": "FAIL",
            "source_revision": "abc",
            "container": "fixture",
            "image": {"image_id": "sha256:test"},
            "acceptance": {
                "prompt_handoffs_count": 1,
                "task_linked_terminal_line_count": 2,
                "prompt_handoff_record_failed_count": 3,
            },
            "cases": [
                {"status": "FAIL", "name": "bad <case>", "detail": "<unsafe>"}
            ],
        }
        html = RUNNER.render_html(report)
        self.assertIn("&lt;unsafe&gt;", html)
        self.assertIn("handoffs=1", html)
        self.assertIn('class="fail"', html)


if __name__ == "__main__":
    unittest.main()
