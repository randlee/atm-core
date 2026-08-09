from __future__ import annotations

import asyncio
import unittest

from hermes_atm import HermesAtmRuntime, HermesAtmRuntimeError


class FakeSession:
    def __init__(self, caller):
        self.caller = caller
        self.callback = None
        self.closed = False

    def activate_receiver(self, _options, callback):
        self.callback = callback

    def snapshot(self):
        return type("Snapshot", (), {"state": "listening"})()

    def close(self):
        self.closed = True


class FakeAdapter:
    def __init__(self):
        self.calls = []

    async def send(self, **kwargs):
        self.calls.append(("send", kwargs))

    async def handle_message(self, event):
        self.calls.append(("handle", event))


class FakeNudge:
    message_id = "01KZMDTEST0000000000000000"
    body = "read atm"


class RuntimeTests(unittest.TestCase):
    def test_missing_environment_fails_before_receiver_activation(self):
        with self.assertRaisesRegex(HermesAtmRuntimeError, "ATM_CHAT_ID"):
            HermesAtmRuntime.from_components(
                adapter=FakeAdapter(),
                loop=asyncio.new_event_loop(),
                event_factory=dict,
                source_factory=dict,
                environment={
                    "ATM_HOME": "/tmp/atm",
                    "ATM_IDENTITY": "skillrx",
                    "ATM_TEAM": "hermes",
                },
            )

    def test_callback_enqueues_notice_then_internal_telegram_event(self):
        """The MVP queues a normal Telegram event; it never uses steer."""

        async def scenario():
            import hermes_atm.runtime as module

            original = module.atm_graft.PyGraftSession
            session = None

            def make_session(caller):
                nonlocal session
                session = FakeSession(caller)
                return session

            module.atm_graft.PyGraftSession = make_session
            try:
                adapter = FakeAdapter()
                loop = asyncio.get_running_loop()
                runtime = HermesAtmRuntime.from_components(
                    adapter=adapter,
                    loop=loop,
                    event_factory=dict,
                    source_factory=dict,
                    environment={
                        "ATM_HOME": "/tmp/atm",
                        "ATM_IDENTITY": "skillrx",
                        "ATM_TEAM": "hermes",
                        "ATM_CHAT_ID": "8991600178",
                    },
                )
                session.callback(FakeNudge())
                await asyncio.sleep(0)
                await asyncio.sleep(0)
                # The real Telegram adapter owns ordering by enqueueing
                # handle_message() on the normal per-session queue.
                self.assertEqual([name for name, _ in adapter.calls], ["send", "handle"])
                self.assertEqual(adapter.calls[0][1]["chat_id"], "8991600178")
                event = adapter.calls[1][1]
                self.assertTrue(event["internal"])
                self.assertEqual(event["text"], "read atm")
                self.assertEqual(event["source"]["platform"], "telegram")
                runtime.close()
                self.assertTrue(session.closed)
            finally:
                module.atm_graft.PyGraftSession = original

        asyncio.run(scenario())


if __name__ == "__main__":
    unittest.main()
