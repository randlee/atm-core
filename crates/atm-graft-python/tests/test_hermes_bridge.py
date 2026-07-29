"""Reference-adapter tests for the checked-in Hermes graft bridge."""

from __future__ import annotations

import ast
from collections import OrderedDict
import inspect
from pathlib import Path
import sys
import textwrap
import unittest

import atm_graft

BRIDGE_ROOT = Path(__file__).resolve().parents[1] / "python"
sys.path.insert(0, str(BRIDGE_ROOT))

from atm_graft_hermes_bridge import HermesGraftBridge, MailboxRecoveryNotice  # noqa: E402


def source(chat_id: str | None = None) -> atm_graft.PyAgentAddress:
    return atm_graft.PyAgentAddress("hendrix", "hermes", chat_id)


def nudge(message_id: str, chat_id: str | None = "1234", body: str = "body") -> atm_graft.PyNudge:
    return atm_graft.PyNudge(message_id, source(chat_id), body)


class FakeGraftSession:
    def __init__(self, *, unread: int = 2, pending_ack: int = 3) -> None:
        self.callback = None
        self.activation_count = 0
        self.unread = unread
        self.pending_ack = pending_ack
        self.count_calls = 0

    def activate_receiver(self, _options: object, callback: object) -> None:
        self.activation_count += 1
        self.callback = callback

    def snapshot(self) -> object:
        return object()

    def close(self) -> None:
        return None

    def mailbox_work_counts(self):
        self.count_calls += 1
        return type("Counts", (), {"unread": self.unread, "pending_ack": self.pending_ack})()


class FakeTimer:
    def __init__(self, callback) -> None:
        self.callback = callback
        self.cancelled = False

    def cancel(self) -> None:
        self.cancelled = True


class FakeLoop:
    def __init__(self) -> None:
        self.calls = []

    def call_later(self, delay, callback):
        timer = FakeTimer(callback)
        self.calls.append((delay, timer))
        return timer


class HermesBridgeTests(unittest.TestCase):
    def make_bridge(
        self,
        delivered: list[atm_graft.PyNudge],
        limit: int = 1_024,
        *,
        session: FakeGraftSession | None = None,
        loop: FakeLoop | None = None,
        recovery_hook=None,
    ) -> HermesGraftBridge:
        bridge = HermesGraftBridge.__new__(HermesGraftBridge)
        bridge._session = session or FakeGraftSession()
        bridge._receiver_options = object()
        bridge._deliver_to_host = delivered.append
        bridge._recent_message_limit = limit
        bridge._recent_message_ids = OrderedDict()
        bridge._recovery_hook = recovery_hook
        bridge._loop = loop or FakeLoop()
        bridge._recovery_timer = None
        return bridge

    def test_start_registers_one_graft_receiver_callback(self) -> None:
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered)

        bridge.start()

        session = bridge._session
        self.assertEqual(session.activation_count, 1)
        self.assertTrue(callable(session.callback))
        self.assertEqual(len(bridge._loop.calls), 1)
        self.assertEqual(bridge._loop.calls[0][0], 10.0)

    def test_recovery_inventory_runs_without_a_hook_and_reports_missing_delivery(self) -> None:
        loop = FakeLoop()
        bridge = self.make_bridge([], loop=loop)

        with self.assertLogs("atm_graft_hermes_bridge", level="ERROR") as logs:
            bridge.start()
            loop.calls[0][1].callback()

        self.assertEqual(bridge._session.count_calls, 1)
        self.assertIn("graft_recovery_hook_missing", "\n".join(logs.output))

    def test_write_is_durable_before_hermes_observes_nudge(self) -> None:
        persisted_ids = {"01KX1TEST00000000000000000"}
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered)
        incoming = nudge("01KX1TEST00000000000000000")

        def inject(received: atm_graft.PyNudge) -> None:
            self.assertIn(incoming.message_id, persisted_ids)
            delivered.append(received)

        bridge._deliver_to_host = inject
        bridge._deliver_nudge(incoming)

        self.assertEqual(delivered, [incoming])

    def test_three_nudges_from_one_qualified_source_use_one_chat(self) -> None:
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered)
        for index in range(3):
            bridge._deliver_nudge(nudge(f"01KX1TEST0000000000000000{index}", body=f"body-{index}"))

        self.assertEqual({str(item.source) for item in delivered}, {"hendrix:1234@hermes"})

    def test_two_chat_ids_are_isolated(self) -> None:
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered)
        bridge._deliver_nudge(nudge("01KX1TEST00000000000000001", "1234", "one"))
        bridge._deliver_nudge(nudge("01KX1TEST00000000000000002", "5678", "two"))

        self.assertEqual(
            [str(item.source) for item in delivered],
            ["hendrix:1234@hermes", "hendrix:5678@hermes"],
        )

    def test_atm_namespace_cannot_collide_with_non_atm_chats(self) -> None:
        key = f"atm:{source('1234')}"
        self.assertNotEqual(key, "telegram:hendrix:1234@hermes")
        self.assertNotEqual(key, "discord:hendrix:1234@hermes")

    def test_malformed_source_fails_closed(self) -> None:
        with self.assertRaises(atm_graft.AtmGraftError):
            atm_graft.PyAgentAddress("hendrix:bad", "hermes", "1234")

    def test_duplicate_nudge_does_not_create_a_second_hermes_turn(self) -> None:
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered)
        incoming = nudge("01KX1TEST00000000000000000")

        bridge._deliver_nudge(incoming)
        bridge._deliver_nudge(incoming)

        self.assertEqual(delivered, [incoming])

    def test_recovery_notice_renders_exact_count_only_text(self) -> None:
        self.assertEqual(
            MailboxRecoveryNotice(2, 3).render(),
            "ATM: 2 unread messages; 3 acknowledgements pending.",
        )

    def test_recovery_timer_is_one_shot_and_cancelled_on_close(self) -> None:
        loop = FakeLoop()
        notices = []
        bridge = self.make_bridge([], loop=loop, recovery_hook=notices.append)

        bridge.start()
        self.assertEqual(len(loop.calls), 1)
        self.assertEqual(loop.calls[0][0], 10.0)
        loop.calls[0][1].callback()
        self.assertEqual(notices, [MailboxRecoveryNotice(2, 3)])
        self.assertEqual(bridge._session.count_calls, 1)
        bridge.close()
        self.assertFalse(loop.calls[0][1].cancelled, "finished timer is not cancelled twice")

    def test_recovery_zero_counts_does_not_inject(self) -> None:
        loop = FakeLoop()
        notices = []
        bridge = self.make_bridge(
            [],
            session=FakeGraftSession(unread=0, pending_ack=0),
            loop=loop,
            recovery_hook=notices.append,
        )

        bridge.start()
        loop.calls[0][1].callback()

        self.assertEqual(notices, [])
        self.assertEqual(bridge._session.count_calls, 1)

    def test_recovery_events_distinguish_empty_work_from_summary_emission(self) -> None:
        loop = FakeLoop()
        empty = self.make_bridge(
            [],
            session=FakeGraftSession(unread=0, pending_ack=0),
            loop=loop,
            recovery_hook=lambda _notice: None,
        )
        with self.assertLogs("atm_graft_hermes_bridge", level="INFO") as empty_logs:
            empty.start()
            loop.calls[0][1].callback()
        self.assertIn("graft_recovery_check_empty", "\n".join(empty_logs.output))
        self.assertNotIn("graft_recovery_summary_emitted", "\n".join(empty_logs.output))

        summary_loop = FakeLoop()
        summary = self.make_bridge(
            [],
            loop=summary_loop,
            recovery_hook=lambda _notice: None,
        )
        with self.assertLogs("atm_graft_hermes_bridge", level="INFO") as summary_logs:
            summary.start()
            summary_loop.calls[0][1].callback()
        self.assertIn("graft_recovery_summary_emitted", "\n".join(summary_logs.output))
        self.assertNotIn("graft_recovery_check_empty", "\n".join(summary_logs.output))

    def test_close_cancels_then_reconnect_schedules_one_new_timer(self) -> None:
        loop = FakeLoop()
        bridge = self.make_bridge([], loop=loop, recovery_hook=lambda _notice: None)

        bridge.start()
        first_timer = loop.calls[0][1]
        bridge.close()
        self.assertTrue(first_timer.cancelled)

        bridge.start()
        self.assertEqual(len(loop.calls), 2)
        self.assertEqual(loop.calls[1][0], 10.0)
        self.assertFalse(loop.calls[1][1].cancelled)

    def test_disconnect_cancels_recovery_window(self) -> None:
        loop = FakeLoop()
        bridge = self.make_bridge([], loop=loop, recovery_hook=lambda _notice: None)

        bridge.start()
        bridge.disconnect()

        self.assertTrue(loop.calls[0][1].cancelled)

    def test_live_nudge_does_not_change_recovery_window(self) -> None:
        loop = FakeLoop()
        notices = []
        delivered: list[atm_graft.PyNudge] = []
        bridge = self.make_bridge(delivered, loop=loop, recovery_hook=notices.append)

        bridge.start()
        timer = loop.calls[0][1]
        bridge._deliver_nudge(nudge("01KX1TEST00000000000000000"))
        self.assertEqual([item.message_id for item in delivered], ["01KX1TEST00000000000000000"])
        self.assertEqual([item.body for item in delivered], ["body"])
        self.assertFalse(timer.cancelled)
        self.assertEqual(len(loop.calls), 1)

        timer.callback()
        self.assertEqual(notices, [MailboxRecoveryNotice(2, 3)])

    def test_recovery_scheduler_has_no_mail_mutation_or_replay_calls(self) -> None:
        scheduler_source = inspect.getsource(HermesGraftBridge._emit_recovery_summary)
        scheduler_tree = ast.parse(textwrap.dedent(scheduler_source))
        call_names = {
            node.func.attr
            for node in ast.walk(scheduler_tree)
            if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
        }
        names = {node.id for node in ast.walk(scheduler_tree) if isinstance(node, ast.Name)}

        self.assertTrue({"read", "acknowledge", "persist", "retry"}.isdisjoint(call_names))
        self.assertNotIn("sqlite", names)


if __name__ == "__main__":
    unittest.main()
