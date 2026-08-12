from __future__ import annotations

import asyncio
import sys
import types
import unittest

from hermes_atm import HermesAtmRuntime, HermesAtmRuntimeError


TEST_SOURCE = "coordinator@test-team"
TEST_PROFILE = "test-profile"
TEST_IDENTITY = "test-agent"
TEST_TEAM = "test-team"
TEST_CHAT_ID = "100000001"


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
    body = f'<atm from="{TEST_SOURCE}"><action>read atm</action></atm>'
    notice_text = f"📬 from {TEST_SOURCE}\nreview failing smoke lane"


class RuntimeTests(unittest.TestCase):
    def test_missing_environment_fails_before_receiver_activation(self):
        with self.assertRaisesRegex(HermesAtmRuntimeError, "ATM_CHAT_ID"):
            HermesAtmRuntime.from_components(
                inject_internal_message=FakeInjector(),
                loop=asyncio.new_event_loop(),
                platform="telegram",
                profile=TEST_PROFILE,
                environment={
                    "ATM_HOME": "/tmp/atm",
                    "ATM_IDENTITY": TEST_IDENTITY,
                    "ATM_TEAM": TEST_TEAM,
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
                    profile=TEST_PROFILE,
                    environment={
                        "ATM_HOME": "/tmp/atm",
                        "ATM_IDENTITY": TEST_IDENTITY,
                        "ATM_TEAM": TEST_TEAM,
                        "ATM_CHAT_ID": TEST_CHAT_ID,
                    },
                )
                session.callback(FakeNudge())
                await asyncio.sleep(0)
                await asyncio.sleep(0)
                self.assertEqual(len(injector.calls), 1)
                call = injector.calls[0]
                self.assertEqual(call["profile"], TEST_PROFILE)
                self.assertEqual(call["platform"], "telegram")
                self.assertEqual(call["chat_id"], TEST_CHAT_ID)
                self.assertEqual(
                    call["text"],
                    f'<atm from="{TEST_SOURCE}"><action>read atm</action></atm>',
                )
                self.assertEqual(call["mode"], "queue")
                self.assertEqual(
                    call["notice_text"],
                    f"📬 from {TEST_SOURCE}\nreview failing smoke lane",
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
                    "ATM_IDENTITY": TEST_IDENTITY,
                    "ATM_TEAM": TEST_TEAM,
                    "ATM_CHAT_ID": TEST_CHAT_ID,
                }
                first = HermesAtmRuntime.from_components(
                    inject_internal_message=injector,
                    loop=asyncio.get_running_loop(),
                    platform="telegram",
                    profile=TEST_PROFILE,
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
                    [
                        (
                            TEST_PROFILE,
                            TEST_CHAT_ID,
                            f'<atm from="{TEST_SOURCE}"><action>read atm</action></atm>',
                        ),
                        ("other-profile", "12345", "second"),
                    ],
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
                    profile=TEST_PROFILE,
                    environment={
                        "ATM_HOME": "/tmp/atm",
                        "ATM_IDENTITY": TEST_IDENTITY,
                        "ATM_TEAM": TEST_TEAM,
                        "ATM_CHAT_ID": TEST_CHAT_ID,
                    },
                )
                sessions[0].callback(FakeNudge())
                await asyncio.sleep(0)
                await asyncio.sleep(0)
                self.assertEqual(len(injector.calls), 1)
                self.assertIs(injector.calls[0]["platform"], FakePlatform.TELEGRAM)
                self.assertEqual(injector.calls[0]["profile"], TEST_PROFILE)
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

    def test_gateway_runner_api_uses_running_loop_when_host_omits_loop_attribute(self):
        async def scenario():
            import hermes_atm.runtime as module

            original_session = module.atm_graft.PyGraftSession
            original_gateway = sys.modules.get("gateway")
            original_config = sys.modules.get("gateway.config")

            class FakePlatform:
                TELEGRAM = object()

            gateway_module = types.ModuleType("gateway")
            config_module = types.ModuleType("gateway.config")
            config_module.Platform = FakePlatform
            gateway_module.config = config_module
            sys.modules["gateway"] = gateway_module
            sys.modules["gateway.config"] = config_module

            def make_session(caller):
                return FakeSession(caller)

            module.atm_graft.PyGraftSession = make_session
            try:
                runner = types.SimpleNamespace(inject_internal_message=FakeInjector())
                runtime = HermesAtmRuntime.from_gateway_runner(
                    runner,
                    profile=TEST_PROFILE,
                    environment={
                        "ATM_HOME": "/tmp/atm",
                        "ATM_IDENTITY": TEST_IDENTITY,
                        "ATM_TEAM": TEST_TEAM,
                        "ATM_CHAT_ID": TEST_CHAT_ID,
                    },
                )
                self.assertIs(runtime.loop, asyncio.get_running_loop())
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
