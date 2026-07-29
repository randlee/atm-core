"""Contract tests for non-interrupting Hermes ATM graft steer delivery."""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
import sys
import threading
import unittest

ADAPTER_ROOT = Path(__file__).resolve().parents[1] / "python"
sys.path.insert(0, str(ADAPTER_ROOT))

from atm_graft_hermes_adapter import (  # noqa: E402
    AtmGraftAdapter,
    HermesRpcSteerPort,
    HermesSteerFailure,
)


@dataclass(frozen=True)
class FakeNudge:
    message_id: str
    source: str
    body: str


@dataclass(frozen=True)
class FakeRecoveryNotice:
    text: str

    def render(self) -> str:
        return self.text


class RecordingSteerPort:
    def __init__(self, failure: Exception | None = None) -> None:
        self.calls: list[tuple[str, str]] = []
        self.failure = failure

    async def steer(self, *, chat_id: str, text: str) -> None:
        self.calls.append((chat_id, text))
        if self.failure is not None:
            raise self.failure


class NoNormalMessageFallbackPort(RecordingSteerPort):
    """A host sentinel: any accidental normal ingress use fails this test."""

    def __init__(self, failure: Exception | None = None) -> None:
        super().__init__(failure)
        self.normal_message_handler_called = False

    async def normal_message_handler(self, _text: str) -> None:
        self.normal_message_handler_called = True
        raise AssertionError("normal ingress must never be called")


class HermesAdapterContractTests(unittest.TestCase):
    def test_adapter_source_forbids_retired_normal_ingress_symbols(self) -> None:
        source = (ADAPTER_ROOT / "atm_graft_hermes_adapter.py").read_text(encoding="utf-8")

        for retired_symbol in (
            "MessageEvent",
            "SessionSource",
            "internal=False",
            "inject_user_message",
            "BasePlatformAdapter",
            "register_platform",
        ):
            with self.subTest(retired_symbol=retired_symbol):
                self.assertNotIn(retired_symbol, source)

    def test_live_nudge_uses_configured_session_and_preserves_source_as_attribution(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="configured-host-session", steer_port=port)
            nudge = FakeNudge("01KX1TEST00000000000000000", "sender:telegram-chat@team", "ATM: new mail")

            await adapter.connect()
            await adapter.deliver_live_nudge(nudge)

            self.assertEqual(port.calls, [("configured-host-session", "ATM: new mail")])
            self.assertEqual(adapter.attribution_for(nudge.message_id).source, "sender:telegram-chat@team")

        asyncio.run(scenario())

    def test_two_source_chat_ids_still_use_one_configured_host_session(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)
            await adapter.deliver_live_nudge(FakeNudge("01KX1TEST00000000000000001", "sender:telegram-chat@team", "one"))
            await adapter.deliver_live_nudge(FakeNudge("01KX1TEST00000000000000002", "sender:future-chat@team", "two"))

            self.assertEqual(port.calls, [("host-session", "one"), ("host-session", "two")])
            self.assertEqual(adapter.attribution_for("01KX1TEST00000000000000002").source, "sender:future-chat@team")

        asyncio.run(scenario())

    def test_adapter_leaves_live_ulid_deduplication_to_the_bridge(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)
            first = FakeNudge("01KX1TEST00000000000000003", "sender@team", "one")
            await adapter.deliver_live_nudge(first)
            await adapter.deliver_live_nudge(first)
            await adapter.deliver_live_nudge(FakeNudge("01KX1TEST00000000000000004", "sender@team", "two"))

            self.assertEqual(
                port.calls,
                [("host-session", "one"), ("host-session", "one"), ("host-session", "two")],
            )
            self.assertFalse(hasattr(adapter, "_recent_message_ids"))

        asyncio.run(scenario())

    def test_source_attribution_is_bounded_without_creating_a_session_registry(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(
                chat_id="host-session",
                steer_port=port,
                attribution_limit=1,
            )
            first = FakeNudge("01KX1TEST00000000000000009", "sender:one@team", "one")
            second = FakeNudge("01KX1TEST00000000000000010", "sender:two@team", "two")

            await adapter.deliver_live_nudge(first)
            await adapter.deliver_live_nudge(second)

            self.assertIsNone(adapter.attribution_for(first.message_id))
            self.assertEqual(adapter.attribution_for(second.message_id).source, "sender:two@team")
            self.assertFalse(hasattr(adapter, "sessions"))

        asyncio.run(scenario())

    def test_recovery_summary_uses_the_same_non_interrupting_steer_port(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)
            await adapter.deliver_recovery_summary(FakeRecoveryNotice("ATM: 2 unread messages; 3 acknowledgements pending."))
            self.assertEqual(port.calls, [("host-session", "ATM: 2 unread messages; 3 acknowledgements pending.")])

        asyncio.run(scenario())

    def test_bridge_callbacks_schedule_live_and_recovery_delivery_on_the_connected_loop(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)
            await adapter.connect()
            adapter.live_nudge_callback(FakeNudge("01KX1TEST00000000000000007", "sender@team", "live"))
            adapter.recovery_summary_callback(FakeRecoveryNotice("recovery"))
            await asyncio.sleep(0)
            await asyncio.sleep(0)
            self.assertEqual(port.calls, [("host-session", "live"), ("host-session", "recovery")])

        asyncio.run(scenario())

    def test_steer_failure_is_structured_visible_and_has_no_normal_message_fallback(self) -> None:
        async def scenario() -> None:
            port = NoNormalMessageFallbackPort(RuntimeError("gateway unavailable"))
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)

            with self.assertRaisesRegex(HermesSteerFailure, "steer_error"):
                await adapter.deliver_live_nudge(FakeNudge("01KX1TEST00000000000000005", "sender@team", "wake"))
            self.assertFalse(port.normal_message_handler_called)
            self.assertEqual(adapter.last_failure.code, "steer_error")
            self.assertEqual(port.calls, [("host-session", "wake")])
            self.assertEqual(
                adapter.attribution_for("01KX1TEST00000000000000005").source,
                "sender@team",
                "source attribution survives a visible failed attempt",
            )
            self.assertTrue(callable(port.normal_message_handler))

        asyncio.run(scenario())

    def test_blank_live_nudge_body_fails_before_the_steer_port(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)

            with self.assertRaisesRegex(HermesSteerFailure, "invalid_text"):
                await adapter.deliver_live_nudge(
                    FakeNudge("01KX1TEST00000000000000011", "sender@team", " ")
                )

            self.assertEqual(port.calls, [])
            self.assertEqual(adapter.last_failure.code, "invalid_text")

        asyncio.run(scenario())

    def test_rpc_port_uses_documented_session_steer_request_and_rejects_nonqueued_result(self) -> None:
        async def scenario() -> None:
            requests: list[tuple[str, dict[str, str]]] = []

            async def request(method: str, params: dict[str, str]) -> dict[str, object]:
                requests.append((method, params))
                return {"result": {"status": "queued", "text": params["text"]}}

            await HermesRpcSteerPort(request).steer(chat_id="host-session", text="wake")
            self.assertEqual(requests, [("session.steer", {"session_id": "host-session", "text": "wake"})])

            async def rejected(_method: str, _params: dict[str, str]) -> dict[str, object]:
                return {"result": {"status": "rejected"}}

            with self.assertRaisesRegex(HermesSteerFailure, "rejected"):
                await HermesRpcSteerPort(rejected).steer(chat_id="host-session", text="wake")

            with self.assertRaisesRegex(HermesSteerFailure, "invalid_text"):
                await HermesRpcSteerPort(request).steer(chat_id="host-session", text=" ")

        asyncio.run(scenario())

    def test_callback_schedules_on_connected_loop_from_a_foreign_thread(self) -> None:
        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="host-session", steer_port=port)
            await adapter.connect()

            worker = threading.Thread(
                target=adapter.live_nudge_callback,
                args=(FakeNudge("01KX1TEST00000000000000008", "sender@team", "threaded"),),
            )
            worker.start()
            worker.join()
            await asyncio.sleep(0)
            await asyncio.sleep(0)

            self.assertEqual(port.calls, [("host-session", "threaded")])

        asyncio.run(scenario())

    def test_scheduled_failure_is_reported_to_the_host_and_cleared_after_success(self) -> None:
        async def scenario() -> None:
            failures: list[HermesSteerFailure] = []
            reported = asyncio.Event()

            def report(failure: HermesSteerFailure) -> None:
                failures.append(failure)
                reported.set()

            port = RecordingSteerPort(RuntimeError("gateway unavailable"))
            adapter = AtmGraftAdapter(
                chat_id="host-session",
                steer_port=port,
                failure_hook=report,
            )
            await adapter.connect()
            adapter.live_nudge_callback(FakeNudge("01KX1TEST00000000000000012", "sender@team", "fail"))
            await asyncio.wait_for(reported.wait(), timeout=1)

            self.assertEqual([failure.code for failure in failures], ["steer_error"])
            self.assertEqual(adapter.last_failure.code, "steer_error")

            port.failure = None
            await adapter.deliver_live_nudge(FakeNudge("01KX1TEST00000000000000013", "sender@team", "pass"))
            self.assertIsNone(adapter.last_failure)

        asyncio.run(scenario())

    def test_blank_configured_chat_id_fails_closed_and_reconnect_does_not_make_registry(self) -> None:
        with self.assertRaisesRegex(ValueError, "ATM_CHAT_ID"):
            AtmGraftAdapter(chat_id=" ", steer_port=RecordingSteerPort())

        async def scenario() -> None:
            port = RecordingSteerPort()
            adapter = AtmGraftAdapter(chat_id="one-profile", steer_port=port)
            await adapter.connect()
            await adapter.connect()
            self.assertEqual(adapter._chat_id, "one-profile")
            self.assertFalse(hasattr(adapter, "sessions"))

        asyncio.run(scenario())


if __name__ == "__main__":
    unittest.main()
