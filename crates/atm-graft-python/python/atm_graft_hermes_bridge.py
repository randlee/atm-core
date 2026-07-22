"""Reference bridge from canonical ATM graft nudges to Hermes user input.

This module is deliberately transport-free. ``atm_graft`` owns the daemon
client and receiver lifecycle; Hermes owns its ordinary inbound-user-message
path. The bridge only maps a typed source to an isolated chat and forwards the
nudge body once.
"""

from __future__ import annotations

from collections import OrderedDict
from collections.abc import Callable
from typing import Any

import atm_graft


_SAFE_SEGMENT = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-")


def _required_segment(value: object, name: str) -> str:
    if not isinstance(value, str) or not value or any(char not in _SAFE_SEGMENT for char in value):
        raise ValueError(f"invalid ATM {name}")
    return value


def _optional_segment(value: object, name: str) -> str | None:
    if value is None:
        return None
    return _required_segment(value, name)


def hermes_chat_key(source: Any) -> str:
    """Map a typed ATM source to its isolated Hermes ``atm:`` chat key.

    This constructs the key from structured fields. It never parses a rendered
    ``agent:chat-id@team`` string.
    """

    agent = _required_segment(getattr(source, "agent", None), "source agent")
    team = _required_segment(getattr(source, "team", None), "source team")
    chat_id = _optional_segment(getattr(source, "chat_id", None), "source chat ID")
    identity = f"{agent}@{team}" if chat_id is None else f"{agent}:{chat_id}@{team}"
    return f"atm:{identity}"


class HermesGraftBridge:
    """One graft receiver bound to one Hermes profile and inbound callback."""

    def __init__(
        self,
        caller: atm_graft.PyAgentAddress,
        receiver_options: atm_graft.PyGraftSessionOptions,
        inject_user_message: Callable[[str, str], None],
        *,
        recent_message_limit: int = 1_024,
    ) -> None:
        if recent_message_limit < 1:
            raise ValueError("recent_message_limit must be positive")
        self._session = atm_graft.PyGraftSession(caller)
        self._receiver_options = receiver_options
        self._inject_user_message = inject_user_message
        self._recent_message_limit = recent_message_limit
        self._recent_message_ids: OrderedDict[str, None] = OrderedDict()

    def start(self) -> None:
        """Activate the one existing graft receiver for this Hermes profile."""

        self._session.activate_receiver(self._receiver_options, self._deliver_nudge)

    def snapshot(self) -> atm_graft.PyGraftSessionSnapshot:
        """Return the existing graft receiver snapshot."""

        return self._session.snapshot()

    def close(self) -> None:
        """Close the existing graft receiver and client."""

        self._session.close()

    def _deliver_nudge(self, nudge: atm_graft.PyNudge) -> None:
        message_id = _required_segment(nudge.message_id, "message ID")
        if message_id in self._recent_message_ids:
            return

        chat_key = hermes_chat_key(nudge.source)
        self._recent_message_ids[message_id] = None
        try:
            self._inject_user_message(chat_key, nudge.body)
        except Exception:
            self._recent_message_ids.pop(message_id, None)
            raise
        while len(self._recent_message_ids) > self._recent_message_limit:
            self._recent_message_ids.popitem(last=False)
