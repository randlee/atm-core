"""Unit checks for the BB.5 live assignment/task-pass runner."""

from __future__ import annotations

import unittest

from run_bb5_assignment import SCENARIO_NAMES, blocks_for


class Bb5AssignmentRunnerTests(unittest.TestCase):
    def test_bb5_scenario_inventory_is_complete_and_ordered(self) -> None:
        self.assertEqual(SCENARIO_NAMES, (
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

        blocks = blocks_for(text, task)

        self.assertEqual(len(blocks), 2)
        self.assertTrue(all(task in block for block in blocks))
        self.assertNotIn(other, "\n".join(blocks))
