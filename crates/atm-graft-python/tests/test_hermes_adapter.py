"""Contract tests for the Hermes ATM platform adapter."""

from __future__ import annotations

import inspect
from pathlib import Path
import os
import sys
import tempfile
import unittest

ADAPTER_ROOT = Path(__file__).resolve().parents[1] / "python"
sys.path.insert(0, str(ADAPTER_ROOT))

from atm_graft_hermes_adapter import AtmGraftAdapter  # noqa: E402


class HermesAdapterContractTests(unittest.TestCase):
    def test_adapter_inherits_hermes_base_contract(self) -> None:
        from gateway.platforms.base import BasePlatformAdapter

        self.assertTrue(issubclass(AtmGraftAdapter, BasePlatformAdapter))
        for name in (
            "set_message_handler",
            "set_fatal_error_handler",
            "set_session_store",
            "set_busy_session_handler",
            "set_topic_recovery_fn",
        ):
            self.assertTrue(callable(getattr(AtmGraftAdapter, name)))

    def test_adapter_implements_gateway_entry_points(self) -> None:
        for name in ("connect", "disconnect", "send", "get_chat_info"):
            self.assertTrue(callable(getattr(AtmGraftAdapter, name)))

    def test_lifecycle_and_send_signatures_match_gateway(self) -> None:
        connect = inspect.signature(AtmGraftAdapter.connect)
        send = inspect.signature(AtmGraftAdapter.send)
        self.assertIn("is_reconnect", connect.parameters)
        self.assertIn("reply_to", send.parameters)
        self.assertIn("metadata", send.parameters)
        self.assertTrue(inspect.iscoroutinefunction(AtmGraftAdapter.get_chat_info))

    def test_chat_id_parser_preserves_agent_chat_and_team(self) -> None:
        with tempfile.TemporaryDirectory() as workspace:
            previous = os.environ.get("ATM_WORKSPACE_ROOT")
            os.environ["ATM_WORKSPACE_ROOT"] = workspace
            try:
                adapter = AtmGraftAdapter(None)
                class FakeGraft:
                    class PyAgentAddress:
                        def __init__(self, agent, team, chat_id):
                            self.agent = agent
                            self.team = team
                            self.chat_id = chat_id

                target = adapter._target_from_chat_id(
                    FakeGraft, "atm:team-lead:chat-42@atm-dev"
                )
                self.assertEqual(
                    (target.agent, target.team, target.chat_id),
                    ("team-lead", "atm-dev", "chat-42"),
                )
                self.assertIsNone(adapter._target_from_chat_id(FakeGraft, "atm"))
            finally:
                if previous is None:
                    os.environ.pop("ATM_WORKSPACE_ROOT", None)
                else:
                    os.environ["ATM_WORKSPACE_ROOT"] = previous

    def test_dispatch_nudge_targets_configured_telegram_session(self) -> None:
        observed = []

        async def handle(event) -> None:
            observed.append((event.source.chat_id, event.source.user_id, event.text))

        adapter = AtmGraftAdapter(None)
        adapter._chat_id = "8991600178"
        adapter.set_message_handler(handle)
        chat_key = "atm:Cipher-311d:8991600178@atm-dev"

        # Run the async dispatch without requiring a live daemon or gateway.
        import asyncio

        asyncio.run(adapter._dispatch_nudge(chat_key, "reply-route-check"))
        self.assertEqual(
            observed,
            [("8991600178", "8991600178", "reply-route-check")],
        )


if __name__ == "__main__":
    unittest.main()
