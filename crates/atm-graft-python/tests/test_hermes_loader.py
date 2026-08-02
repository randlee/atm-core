"""Production Hermes graft composition-seam tests."""

from __future__ import annotations

import asyncio
import unittest

import atm_graft
from atm_graft_hermes_loader import HermesGraftRuntime


class LoaderSession:
    def __init__(self) -> None:
        self.activation_count = 0
        self.close_count = 0
        self.callback = None

    def activate_receiver(self, _options, callback) -> None:
        self.activation_count += 1
        self.callback = callback

    def snapshot(self):
        return type("Snapshot", (), {"state": "listening"})()

    def close(self) -> None:
        self.close_count += 1


class HermesLoaderTests(unittest.TestCase):
    def test_environment_loader_constructs_starts_and_closes_one_profile(self) -> None:
        async def scenario() -> None:
            requests: list[tuple[str, dict[str, str]]] = []
            sessions: list[LoaderSession] = []

            async def request(method: str, params: dict[str, str]):
                requests.append((method, params))
                return {"result": {"status": "queued"}}

            async def resolve(chat_id: str) -> str:
                self.assertEqual(chat_id, "8991600178")
                return "runtime-session-uuid"

            def session_factory(_caller: atm_graft.PyAgentAddress) -> LoaderSession:
                session = LoaderSession()
                sessions.append(session)
                return session

            runtime = HermesGraftRuntime.from_environment(
                environment={
                    "ATM_HOME": "/tmp/hermes",
                    "ATM_IDENTITY": "skillrx",
                    "ATM_TEAM": "hermes",
                    "ATM_CHAT_ID": "8991600178",
                },
                request=request,
                resolve_session_id=resolve,
                session_factory=session_factory,
            )

            snapshot = await runtime.start()
            self.assertEqual(snapshot.state, "listening")
            self.assertEqual(sessions[0].activation_count, 1)
            self.assertIsNotNone(sessions[0].callback)

            sessions[0].callback(
                atm_graft.PyNudge(
                    "01KX1TEST00000000000000000",
                    atm_graft.PyAgentAddress("hendrix", "hermes", "1234"),
                    "wake",
                )
            )
            await asyncio.sleep(0)
            await asyncio.sleep(0)
            self.assertEqual(
                requests,
                [("session.steer", {"session_id": "runtime-session-uuid", "text": "wake"})],
            )

            runtime.close()
            self.assertEqual(sessions[0].close_count, 1)

        asyncio.run(scenario())

    def test_environment_loader_requires_profile_chat_id(self) -> None:
        with self.assertRaisesRegex(ValueError, "ATM_CHAT_ID"):
            HermesGraftRuntime.from_environment(
                environment={
                    "ATM_HOME": "/tmp/hermes",
                    "ATM_IDENTITY": "skillrx",
                    "ATM_TEAM": "hermes",
                },
                request=lambda _method, _params: None,
                resolve_session_id=lambda _chat_id: "runtime-session-uuid",
            )


if __name__ == "__main__":
    unittest.main()
