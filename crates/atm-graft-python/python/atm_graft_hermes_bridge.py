"""Reference bridge from canonical ATM graft nudges to Hermes user input.

This module is deliberately transport-free. ``atm_graft`` owns the daemon
client and receiver lifecycle; Hermes owns its ordinary inbound-user-message
path. The bridge only maps a typed source to an isolated chat and forwards the
nudge body once.
"""

from __future__ import annotations

from collections import OrderedDict
from collections.abc import Callable
import atm_graft


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
        message_id = nudge.message_id
        if message_id in self._recent_message_ids:
            return

        chat_key = f"atm:{nudge.source}"
        self._recent_message_ids[message_id] = None
        try:
            self._inject_user_message(chat_key, nudge.body)
        except Exception:
            self._recent_message_ids.pop(message_id, None)
            raise
        while len(self._recent_message_ids) > self._recent_message_limit:
            self._recent_message_ids.popitem(last=False)
