from __future__ import annotations

import asyncio
import sys
import types
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


class FakeInjector:
    def __init__(self):
        self.calls = []

    async def __call__(self, **kwargs):
        self.calls.append(kwargs)


class FakeNudge:
    message_id = "01KZMDTEST0000000000000000"
    body = "read atm"


class RuntimeTests(unittest.TestCase):
    def test_missing_environment_fails_before_receiver_activation(self):
        with self.assertRaisesRegex(HermesAtmRuntimeError, "ATM_CHAT_ID"):
            HermesAtmRuntime.from_components(
                inject_internal_message=FakeInjector(),
                loop=asyncio.new_event_loop(),
                platform="telegram",
                profile="skillrx",
                environment={
                    "ATM_HOME": "/tmp/atm",
                    "ATM_IDENTITY": "skillrx",
                    "ATM_TEAM": "hermes",
                },
            )

    def test_callback_enqueues_notice_then_internal_telegram_event(self):
        """The runner API owns Hermes' existing internal-event queue seam.

        ATM steer is not implemented in this MVP and remains a planned future
        delivery mode.
        """

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
                injector = FakeInjector()
                loop = asyncio.get_running_loop()
                runtime = HermesAtmRuntime.from_components(
                    inject_internal_message=injector,
                    loop=loop,
                    platform="telegram",
                    profile="skillrx",
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
                self.assertEqual(len(injector.calls), 1)
                call = injector.calls[0]
                self.assertEqual(call["profile"], "skillrx")
                self.assertEqual(call["platform"], "telegram")
                self.assertEqual(call["chat_id"], "8991600178")
                self.assertEqual(call["text"], "read atm")
                self.assertEqual(call["mode"], "queue")
                self.assertEqual(
                    call["notice_text"],
                    "📬 ATM nudge received; routing through your existing Telegram session.",
                )
                runtime.close()
                self.assertTrue(session.closed)
            finally:
                module.atm_graft.PyGraftSession = original

        asyncio.run(scenario())

    def test_distinct_profile_and_chat_compositions_do_not_bleed(self):
        async def scenario():
            import hermes_atm.runtime as module

            original = module.atm_graft.PyGraftSession
            sessions = []

            def make_session(caller):
                session = FakeSession(caller)
                sessions.append(session)
                return session

            module.atm_graft.PyGraftSession = make_session
            try:
                injector = FakeInjector()
                environment = {
                    "ATM_HOME": "/tmp/atm",
                    "ATM_IDENTITY": "skillrx",
                    "ATM_TEAM": "hermes",
                    "ATM_CHAT_ID": "8991600178",
                }
                first = HermesAtmRuntime.from_components(
                    inject_internal_message=injector,
                    loop=asyncio.get_running_loop(),
                    platform="telegram",
                    profile="skillrx",
                    environment=environment,
                )
                second = HermesAtmRuntime.from_components(
                    inject_internal_message=injector,
                    loop=asyncio.get_running_loop(),
                    platform="telegram",
                    profile="other-profile",
                    environment={**environment, "ATM_CHAT_ID": "12345"},
                )
                sessions[0].callback(FakeNudge())
                sessions[1].callback(type("Nudge", (), {"body": "second"})())
                await asyncio.sleep(0)
                await asyncio.sleep(0)
                self.assertEqual(
                    [(call["profile"], call["chat_id"], call["text"]) for call in injector.calls],
                    [("skillrx", "8991600178", "read atm"), ("other-profile", "12345", "second")],
                )
                first.close()
                second.close()
            finally:
                module.atm_graft.PyGraftSession = original

        asyncio.run(scenario())

    def test_gateway_runner_api_receives_explicit_profile_and_telegram_platform(self):
        async def scenario():
            import hermes_atm.runtime as module

            original_session = module.atm_graft.PyGraftSession
            original_gateway = sys.modules.get("gateway")
            original_config = sys.modules.get("gateway.config")
            sessions = []

            class FakePlatform:
                TELEGRAM = object()

            gateway_module = types.ModuleType("gateway")
            config_module = types.ModuleType("gateway.config")
            config_module.Platform = FakePlatform
            gateway_module.config = config_module
            sys.modules["gateway"] = gateway_module
            sys.modules["gateway.config"] = config_module

            def make_session(caller):
                session = FakeSession(caller)
                sessions.append(session)
                return session

            module.atm_graft.PyGraftSession = make_session
            try:
                injector = FakeInjector()
                runner = types.SimpleNamespace(
                    gateway_loop=asyncio.get_running_loop(),
                    inject_internal_message=injector,
                )
                runtime = HermesAtmRuntime.from_gateway_runner(
                    runner,
                    profile="skillrx",
                    environment={
                        "ATM_HOME": "/tmp/atm",
                        "ATM_IDENTITY": "skillrx",
                        "ATM_TEAM": "hermes",
                        "ATM_CHAT_ID": "8991600178",
                    },
                )
                sessions[0].callback(FakeNudge())
                await asyncio.sleep(0)
                await asyncio.sleep(0)
                self.assertEqual(len(injector.calls), 1)
                self.assertIs(injector.calls[0]["platform"], FakePlatform.TELEGRAM)
                self.assertEqual(injector.calls[0]["profile"], "skillrx")
                runtime.close()
            finally:
                module.atm_graft.PyGraftSession = original_session
                if original_gateway is None:
                    sys.modules.pop("gateway", None)
                else:
                    sys.modules["gateway"] = original_gateway
                if original_config is None:
                    sys.modules.pop("gateway.config", None)
                else:
                    sys.modules["gateway.config"] = original_config

        asyncio.run(scenario())


if __name__ == "__main__":
    unittest.main()
