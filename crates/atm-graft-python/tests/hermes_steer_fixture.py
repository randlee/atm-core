"""Checked-in reference profile for deterministic Hermes steer evidence.

It models only the supported safe-boundary contract: a steer is accepted while
one tool is active, then becomes visible after that tool completes.  It does
not model a normal inbound message, an interrupt, or an ATM mailbox.
"""

from __future__ import annotations

import asyncio
from collections import OrderedDict
from dataclasses import dataclass
from pathlib import Path
import sys
from types import ModuleType

PYTHON_SOURCE = Path(__file__).resolve().parents[1] / "python"
if str(PYTHON_SOURCE) not in sys.path:
    sys.path.insert(0, str(PYTHON_SOURCE))

from atm_graft_hermes_adapter import AtmGraftAdapter  # noqa: E402

# The fixture executes the real bridge source without requiring its compiled
# pyo3 extension. It configures the bridge through ``__new__`` below, so only
# its postponed annotations need this minimal import placeholder.
if "atm_graft" not in sys.modules:
    sys.modules["atm_graft"] = ModuleType("atm_graft")

from atm_graft_hermes_bridge import HermesGraftBridge  # noqa: E402


@dataclass(frozen=True)
class FixtureNudge:
    message_id: str
    source: str
    body: str


@dataclass(frozen=True)
class FixtureRecoveryNotice:
    unread: int
    pending_ack: int

    def render(self) -> str:
        return f"ATM: {self.unread} unread messages; {self.pending_ack} acknowledgements pending."


class FixtureTimer:
    def __init__(self, callback) -> None:
        self.callback = callback
        self.cancelled = False

    def cancel(self) -> None:
        self.cancelled = True


class FixtureLoop:
    def __init__(self) -> None:
        self.calls: list[tuple[float, FixtureTimer]] = []

    def call_later(self, delay: float, callback) -> FixtureTimer:
        timer = FixtureTimer(callback)
        self.calls.append((delay, timer))
        return timer


class FixtureGraftSession:
    def __init__(self, *, unread: int = 2, pending_ack: int = 3) -> None:
        self.unread = unread
        self.pending_ack = pending_ack
        self.activation_count = 0
        self.count_calls = 0

    def activate_receiver(self, _options: object, _callback: object) -> None:
        self.activation_count += 1

    def mailbox_work_counts(self):
        self.count_calls += 1
        return type("Counts", (), {"unread": self.unread, "pending_ack": self.pending_ack})()


class HermesSteerFixture:
    """A one-session, active-tool reference host implementing ``HermesSteerPort``."""

    def __init__(self, *, profile: str = "agent@team", chat_id: str = "configured-host-session") -> None:
        self.profile = profile
        self.chat_id = chat_id
        self.current_task = "active-tool-call"
        self.current_task_interrupted = False
        self.normal_message_handler_called = False
        self.mailbox_mutated_by_wake = False
        self._pending_steers: list[str] = []
        self.visible_after_safe_boundary: list[str] = []
        self.adapter = AtmGraftAdapter(chat_id=chat_id, steer_port=self)
        self._bridge_session = FixtureGraftSession()
        self._bridge_loop = FixtureLoop()
        self._bridge = HermesGraftBridge.__new__(HermesGraftBridge)
        self._bridge._session = self._bridge_session
        self._bridge._receiver_options = object()
        self._bridge._deliver_to_host = self.adapter.live_nudge_callback
        self._bridge._recent_message_limit = 1_024
        self._bridge._recent_message_ids = OrderedDict()
        self._bridge._recovery_hook = self.adapter.recovery_summary_callback
        self._bridge._loop = self._bridge_loop
        self._bridge._recovery_timer = None

    async def steer(self, *, chat_id: str, text: str) -> None:
        if chat_id != self.chat_id:
            raise RuntimeError("steer targeted a different Hermes session")
        if not self.current_task:
            raise RuntimeError("fixture requires an active safe boundary")
        self._pending_steers.append(text)

    def finish_current_tool_boundary(self) -> None:
        """The sole place accepted steer text becomes observable to the agent."""

        self.visible_after_safe_boundary.extend(self._pending_steers)
        self._pending_steers.clear()

    async def prove_live_nudge(self) -> dict[str, object]:
        await self.adapter.connect()
        text = "ATM: live nudge available."
        self._bridge.start()
        self._bridge._deliver_nudge(
            FixtureNudge("01KX1TEST00000000000000006", "sender:telegram-chat@team", text)
        )
        await asyncio.sleep(0)
        await asyncio.sleep(0)
        if self._bridge_session.count_calls != 0:
            raise AssertionError("live nudge must not run the delayed recovery inventory")
        self._assert_safe_before_boundary()
        self.finish_current_tool_boundary()
        return self._evidence("live_nudge", text)

    async def prove_recovery_summary(self) -> dict[str, object]:
        await self.adapter.connect()
        notice = FixtureRecoveryNotice(2, 3)
        text = notice.render()
        self._bridge.start()
        if self._bridge_loop.calls[0][0] != 10.0:
            raise AssertionError("recovery inventory must be scheduled exactly ten seconds after activation")
        self._bridge_loop.calls[0][1].callback()
        await asyncio.sleep(0)
        await asyncio.sleep(0)
        if self._bridge_session.count_calls != 1:
            raise AssertionError("recovery inventory must read durable mailbox counts exactly once")
        self._assert_safe_before_boundary()
        self.finish_current_tool_boundary()
        return self._evidence("recovery_summary", text)

    def _assert_safe_before_boundary(self) -> None:
        if self.visible_after_safe_boundary:
            raise AssertionError("steer became visible before the safe boundary")
        if self.current_task_interrupted or self.normal_message_handler_called:
            raise AssertionError("fixture observed an interrupting normal message path")
        if self.mailbox_mutated_by_wake:
            raise AssertionError("wake-up mutated the ATM mailbox")

    def _evidence(self, wake_kind: str, text: str) -> dict[str, object]:
        if self.visible_after_safe_boundary != [text]:
            raise AssertionError("accepted steer was not visible at the safe boundary")
        return {
            "profile": self.profile,
            "chat_id": self.chat_id,
            "wake_kind": wake_kind,
            "steer_accepted": True,
            "normal_message_handler_called": self.normal_message_handler_called,
            "current_task_interrupted": self.current_task_interrupted,
            "mailbox_mutated_by_wake": self.mailbox_mutated_by_wake,
        }


async def fixture_evidence() -> list[dict[str, object]]:
    """Return independent live and delayed-recovery reference evidence rows."""

    return [
        await HermesSteerFixture().prove_live_nudge(),
        await HermesSteerFixture().prove_recovery_summary(),
    ]


def main() -> None:
    """Allow direct fixture execution during focused debugging."""

    import json

    print(json.dumps(asyncio.run(fixture_evidence()), sort_keys=True))


if __name__ == "__main__":
    main()
