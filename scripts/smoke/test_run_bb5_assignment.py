"""Unit checks for the BB.5 live assignment/task-pass runner."""

from __future__ import annotations

import unittest
from unittest import mock

from scripts.smoke import run_bb5_assignment as RUNNER


class Bb5AssignmentRunnerTests(unittest.TestCase):
    def test_bb5_scenario_inventory_is_complete_and_ordered(self) -> None:
        self.assertEqual(RUNNER.SCENARIO_NAMES, (
            "three_assignments_show_queued_1_2_3_then_one_ready",
            "pending_ack_header_stays_zero_across_assignments",
            "close_shows_ready_for_next_task_within_one_pass",
            "reassign_shows_closed_reassigned_to_old_and_queued_to_new",
            "move_to_head_while_idle_shows_ready_next_pass_and_no_extra_line",
            "cancel_shows_closed_cancelled_to_assignee",
            "busy_assigner_receives_started_and_complete_at_write_time",
        ))


    def test_blocks_for_keeps_each_task_message_separate(self) -> None:
        task = "BB5-example-one"
        other = "BB5-example-two"
        text = (
            f'<atm task="{task}" queued="1" message="queued" />\n'
            f'<atm task="{other}" queued="2" message="other" />\n'
            f'<atm task="{task}" ready message="ready" />\n'
        )

        blocks = RUNNER.blocks_for(text, task)

        self.assertEqual(len(blocks), 2)
        self.assertTrue(all(task in block for block in blocks))
        self.assertNotIn(other, "\n".join(blocks))

    def test_json_value_rejects_failed_and_malformed_cli_results(self) -> None:
        with self.assertRaisesRegex(RuntimeError, "member list failed"):
            RUNNER.json_value({"exit_code": 1, "stdout": "", "stderr": "member list failed"}, "member list")
        with self.assertRaisesRegex(RuntimeError, "did not return JSON"):
            RUNNER.json_value({"exit_code": 0, "stdout": "not-json", "stderr": ""}, "member list")

    def test_task_rows_rejects_non_object_and_non_list_payloads(self) -> None:
        self.assertEqual(RUNNER.task_rows(None), [])
        self.assertEqual(RUNNER.task_rows({"rows": ["bad", 1]}), [])
        self.assertEqual(RUNNER.task_rows({"rows": [{"task_id": "ok"}, "bad"]}), [{"task_id": "ok"}])

    def test_agent_pane_rejects_a_roster_without_the_assignee(self) -> None:
        result = {"exit_code": 0, "stdout": '{"result":{"agents":[]}}', "stderr": ""}
        with mock.patch.object(RUNNER, "run_herdr", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "no pane"):
                RUNNER.agent_pane("fixture")

    def test_wait_for_pane_reports_timeout_when_predicate_never_matches(self) -> None:
        result = {"exit_code": 0, "stdout": "unchanged", "stderr": ""}
        with mock.patch.object(RUNNER, "WAIT_SECONDS", 0), mock.patch.object(RUNNER, "pane_text", return_value=result):
            with self.assertRaisesRegex(RuntimeError, "timed out waiting for ready"):
                RUNNER.wait_for_pane("fixture", lambda _text: False, "ready")

    def test_cleanup_tasks_retains_failed_close_results_and_drains_mailboxes(self) -> None:
        failed = {"exit_code": 1, "stdout": "", "stderr": "close failed"}
        with mock.patch.object(RUNNER, "close", return_value=failed), mock.patch.object(
            RUNNER, "drain_mailbox", return_value=[]
        ) as drain:
            results = RUNNER.cleanup_tasks("fixture", ["task-1", "task-1"])
        self.assertEqual(results, [{"task_id": "task-1", "result": failed}])
        self.assertEqual([call.args for call in drain.call_args_list], [("fixture", RUNNER.ASSIGNEE), ("fixture", "stub-beta")])

    def test_inspect_container_surfaces_docker_inspect_failure(self) -> None:
        failed = {"exit_code": 1, "stdout": "", "stderr": "container missing"}
        with mock.patch.object(RUNNER, "run_command", return_value=failed):
            with self.assertRaisesRegex(RuntimeError, "container missing"):
                RUNNER.inspect_container("missing")

    def test_render_html_marks_a_failed_case_and_escapes_its_detail(self) -> None:
        html = RUNNER.render_html({
            "status": "FAIL", "source_revision": "abc", "container": "fixture",
            "image": {"image_id": "sha256:test"}, "roster": [],
            "cases": [{"status": "FAIL", "name": "bad <case>", "detail": "<unsafe>"}],
        })
        self.assertIn('class="fail"', html)
        self.assertIn("✗", html)
        self.assertIn("&lt;unsafe&gt;", html)

    def test_scenario_records_fail_when_an_assignment_command_fails(self) -> None:
        failed = {"exit_code": 1, "stdout": "", "stderr": "assignment rejected"}
        read = {"exit_code": 0, "stdout": '{"bucket_counts":{"unread":0,"pending_ack":0}}', "stderr": ""}
        with mock.patch.object(RUNNER, "assign", return_value=failed), mock.patch.object(
            RUNNER, "run_cli", return_value=read
        ), mock.patch.object(RUNNER, "cleanup_tasks"):
            result = RUNNER.scenario_pending_ack("fixture", "20260913T000000Z")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_three_assignments_reports_failure_on_bad_pane_state(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        pane = {"exit_code": 0, "stdout": (
            '<atm task="one" queued="1" message="queued" />\n'
            '<atm task="two" queued="2" message="queued" />\n'
            '<atm task="three" queued="3" message="queued" />\n'
        ), "stderr": ""}
        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "assign", return_value=success
        ), mock.patch.object(RUNNER, "wait_for_pane", return_value=pane), mock.patch.object(RUNNER, "cleanup_tasks"):
            result = RUNNER.scenario_three_assignments("fixture", "run")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_close_ready_reports_failure_on_missing_ready_line(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        panes = [
            {"exit_code": 0, "stdout": '<atm task="one" ready message="ready" />', "stderr": ""},
            {"exit_code": 0, "stdout": '<atm task="two" message="queued" />', "stderr": ""},
        ]
        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "assign", return_value=success
        ), mock.patch.object(RUNNER, "run_cli", return_value=success), mock.patch.object(
            RUNNER, "wait_for_pane", side_effect=panes
        ), mock.patch.object(RUNNER, "cleanup_tasks"), mock.patch.object(RUNNER, "drain_mailbox"):
            result = RUNNER.scenario_close_ready("fixture", "run")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_reassign_reports_failure_on_bad_ledger_owner(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        pane = {"exit_code": 0, "stdout": '<atm task="reassign" outcome="reassigned" />', "stderr": ""}
        listing = {"exit_code": 0, "stdout": '{"rows":[{"task_id":"reassign","assignee":"tester","position":2}]}', "stderr": ""}
        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "assign", return_value=success
        ), mock.patch.object(RUNNER, "run_cli", side_effect=[success, listing]), mock.patch.object(
            RUNNER, "wait_for_pane", side_effect=[pane, pane]
        ), mock.patch.object(RUNNER, "cleanup_tasks"):
            result = RUNNER.scenario_reassign("fixture", "run")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_move_head_reports_failure_on_bad_transition_line(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        queue = {"exit_code": 0, "stdout": (
            '<atm task="one" queued="1" />\n<atm task="two" queued="2" />\n<atm task="three" queued="3" />'
        ), "stderr": ""}
        bad_ready = {"exit_code": 0, "stdout": '<atm task="three" ready outcome="reassigned" />', "stderr": ""}
        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "assign", return_value=success
        ), mock.patch.object(RUNNER, "run_cli", return_value=success), mock.patch.object(
            RUNNER, "wait_for_pane", side_effect=[queue, bad_ready]
        ), mock.patch.object(RUNNER, "cleanup_tasks"):
            result = RUNNER.scenario_move_head("fixture", "run")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_cancel_reports_failure_on_wrong_terminal_outcome(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        bad_terminal = {"exit_code": 0, "stdout": '<atm task="cancel" outcome="completed" />', "stderr": ""}
        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "assign", return_value=success
        ), mock.patch.object(RUNNER, "close", return_value=success), mock.patch.object(
            RUNNER, "wait_for_pane", return_value=bad_terminal
        ), mock.patch.object(RUNNER, "drain_mailbox"):
            result = RUNNER.scenario_cancel("fixture", "run")
        self.assertEqual(result["status"], "FAIL")

    def test_scenario_busy_assigner_reports_failure_on_missing_complete_notice(self) -> None:
        success = {"exit_code": 0, "stdout": "", "stderr": ""}
        mailbox = {"exit_code": 0, "stdout": '{"rows":[{"task_id":"busy","message_id":"msg-1"}]}', "stderr": ""}
        notice = {"exit_code": 0, "stdout": '{"message":{"taskOp":{"op":"start"}}}', "stderr": ""}

        def cli(_container: str, _identity: str, args: list[str]) -> dict[str, object]:
            if args[:2] == ["list", "--json"]:
                return mailbox
            if args[:2] == ["read", "--message-id"]:
                return notice
            return success

        with mock.patch.object(RUNNER, "task_id", side_effect=lambda _run, suffix, _group: suffix), mock.patch.object(
            RUNNER, "run_cli", side_effect=cli
        ), mock.patch.object(RUNNER, "cleanup_tasks"), mock.patch.object(RUNNER, "drain_mailbox"):
            result = RUNNER.scenario_busy_assigner("fixture", "run")
        self.assertEqual(result["status"], "FAIL")
