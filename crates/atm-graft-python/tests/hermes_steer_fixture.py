"""Checked-in reference profile for deterministic Hermes steer evidence.

It models only the supported safe-boundary contract: a steer is accepted while
one tool is active, then becomes visible after that tool completes.  It does
not model a normal inbound message, an interrupt, or an ATM mailbox.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from pathlib import Path
import sys

PYTHON_SOURCE = Path(__file__).resolve().parents[1] / "python"
if str(PYTHON_SOURCE) not in sys.path:
    sys.path.insert(0, str(PYTHON_SOURCE))

from atm_graft_hermes_adapter import AtmGraftAdapter  # noqa: E402


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
        await self.adapter.deliver_live_nudge(
            FixtureNudge("01KX1TEST00000000000000006", "sender:telegram-chat@team", text)
        )
        self._assert_safe_before_boundary()
        self.finish_current_tool_boundary()
        return self._evidence("live_nudge", text)

    async def prove_recovery_summary(self) -> dict[str, object]:
        await self.adapter.connect()
        notice = FixtureRecoveryNotice(2, 3)
        text = notice.render()
        await self.adapter.deliver_recovery_summary(notice)
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
