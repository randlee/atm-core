"""Reference-adapter tests for the checked-in Hermes graft bridge."""

from __future__ import annotations

from collections import OrderedDict
from pathlib import Path
import sys
import unittest

import atm_graft

BRIDGE_ROOT = Path(__file__).resolve().parents[1] / "python"
sys.path.insert(0, str(BRIDGE_ROOT))

from atm_graft_hermes_bridge import HermesGraftBridge  # noqa: E402


def source(chat_id: str | None = None) -> atm_graft.PyAgentAddress:
    return atm_graft.PyAgentAddress("hendrix", "hermes", chat_id)


def nudge(message_id: str, chat_id: str | None = "1234", body: str = "body") -> atm_graft.PyNudge:
    return atm_graft.PyNudge(message_id, source(chat_id), body)


class FakeGraftSession:
    def __init__(self) -> None:
        self.callback = None
        self.activation_count = 0

    def activate_receiver(self, _options: object, callback: object) -> None:
        self.activation_count += 1
        self.callback = callback

    def snapshot(self) -> object:
        return object()

    def close(self) -> None:
        return None


class HermesBridgeTests(unittest.TestCase):
    def make_bridge(self, delivered: list[tuple[str, str]], limit: int = 1_024) -> HermesGraftBridge:
        bridge = HermesGraftBridge.__new__(HermesGraftBridge)
        bridge._session = FakeGraftSession()
        bridge._receiver_options = object()
        bridge._inject_user_message = lambda chat, body: delivered.append((chat, body))
        bridge._recent_message_limit = limit
        bridge._recent_message_ids = OrderedDict()
        return bridge

    def test_start_registers_one_graft_receiver_callback(self) -> None:
        delivered: list[tuple[str, str]] = []
        bridge = self.make_bridge(delivered)

        bridge.start()

        session = bridge._session
        self.assertEqual(session.activation_count, 1)
        self.assertTrue(callable(session.callback))

    def test_write_is_durable_before_hermes_observes_nudge(self) -> None:
        persisted_ids = {"01KX1TEST00000000000000000"}
        delivered: list[tuple[str, str]] = []
        bridge = self.make_bridge(delivered)
        incoming = nudge("01KX1TEST00000000000000000")

        def inject(chat: str, body: str) -> None:
            self.assertIn(incoming.message_id, persisted_ids)
            delivered.append((chat, body))

        bridge._inject_user_message = inject
        bridge._deliver_nudge(incoming)

        self.assertEqual(delivered, [("atm:hendrix:1234@hermes", "body")])

    def test_three_nudges_from_one_qualified_source_use_one_chat(self) -> None:
        delivered: list[tuple[str, str]] = []
        bridge = self.make_bridge(delivered)
        for index in range(3):
            bridge._deliver_nudge(nudge(f"01KX1TEST0000000000000000{index}", body=f"body-{index}"))

        self.assertEqual({chat for chat, _body in delivered}, {"atm:hendrix:1234@hermes"})

    def test_two_chat_ids_are_isolated(self) -> None:
        delivered: list[tuple[str, str]] = []
        bridge = self.make_bridge(delivered)
        bridge._deliver_nudge(nudge("01KX1TEST00000000000000001", "1234", "one"))
        bridge._deliver_nudge(nudge("01KX1TEST00000000000000002", "5678", "two"))

        self.assertEqual(
            [chat for chat, _body in delivered],
            ["atm:hendrix:1234@hermes", "atm:hendrix:5678@hermes"],
        )

    def test_atm_namespace_cannot_collide_with_non_atm_chats(self) -> None:
        key = f"atm:{source('1234')}"
        self.assertNotEqual(key, "telegram:hendrix:1234@hermes")
        self.assertNotEqual(key, "discord:hendrix:1234@hermes")

    def test_malformed_source_fails_closed(self) -> None:
        with self.assertRaises(atm_graft.AtmGraftError):
            atm_graft.PyAgentAddress("hendrix:bad", "hermes", "1234")

    def test_duplicate_nudge_does_not_create_a_second_hermes_turn(self) -> None:
        delivered: list[tuple[str, str]] = []
        bridge = self.make_bridge(delivered)
        incoming = nudge("01KX1TEST00000000000000000")

        bridge._deliver_nudge(incoming)
        bridge._deliver_nudge(incoming)

        self.assertEqual(delivered, [("atm:hendrix:1234@hermes", "body")])


if __name__ == "__main__":
    unittest.main()
