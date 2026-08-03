"""Reference bridge from canonical ATM graft nudges to a host callback.

This module is deliberately transport-free. ``atm_graft`` owns the daemon
client and receiver lifecycle. The bridge forwards each typed nudge once to
its host callback; the host owns its safe-boundary delivery semantics.
"""

from __future__ import annotations

from collections import OrderedDict
from collections.abc import Callable
from dataclasses import dataclass
import logging
import atm_graft


LOGGER = logging.getLogger(__name__)
RECOVERY_DELAY_SECONDS = 10.0


@dataclass(frozen=True)
class MailboxRecoveryNotice:
    """One bounded recovery prompt derived from daemon mailbox counts."""

    unread: int
    pending_ack: int

    def render(self) -> str:
        return f"ATM: {self.unread} unread messages; {self.pending_ack} acknowledgements pending."


class HermesGraftBridge:
    """One graft receiver bound to one Hermes profile and inbound callback."""

    def __init__(
        self,
        caller: atm_graft.PyAgentAddress,
        receiver_options: atm_graft.PyGraftSessionOptions,
        deliver_nudge: Callable[[atm_graft.PyNudge], None],
        *,
        recent_message_limit: int = 1_024,
        recovery_hook: Callable[[MailboxRecoveryNotice], None],
        loop: object | None = None,
        session: object | None = None,
    ) -> None:
        if recent_message_limit < 1:
            raise ValueError("recent_message_limit must be positive")
        # ``session`` is the lifecycle-owned binding supplied by the host
        # composition seam.  The default keeps the public bridge convenient
        # for direct embedding while tests and host loaders can exercise the
        # complete activate/close lifecycle without replacing module globals.
        self._session = session if session is not None else atm_graft.PyGraftSession(caller)
        self._receiver_options = receiver_options
        self._deliver_to_host = deliver_nudge
        self._recent_message_limit = recent_message_limit
        self._recent_message_ids: OrderedDict[str, None] = OrderedDict()
        self._recovery_hook = recovery_hook
        self._loop = loop
        self._recovery_timer = None

    def start(self) -> None:
        """Activate the one existing graft receiver for this Hermes profile."""

        self._session.activate_receiver(self._receiver_options, self._deliver_nudge)
        snapshot = self._session.snapshot()
        if getattr(snapshot, "state", None) != "listening":
            LOGGER.info(
                "graft_recovery_not_scheduled state=%s",
                getattr(snapshot, "state", "unknown"),
            )
            return
        loop = self._loop
        if loop is None:
            import asyncio
            loop = asyncio.get_running_loop()
        self._cancel_recovery_timer()
        self._recovery_timer = loop.call_later(RECOVERY_DELAY_SECONDS, self._emit_recovery_summary)
        LOGGER.info("graft_recovery_scheduled delay_seconds=%s", RECOVERY_DELAY_SECONDS)

    def snapshot(self) -> atm_graft.PyGraftSessionSnapshot:
        """Return the existing graft receiver snapshot."""

        return self._session.snapshot()

    def close(self) -> None:
        """Close the existing graft receiver and client."""
        self._cancel_recovery_timer()
        self._session.close()

    def disconnect(self) -> None:
        """Cancel recovery work before the host tears down this bridge."""
        self.close()

    def _cancel_recovery_timer(self) -> None:
        if self._recovery_timer is not None:
            self._recovery_timer.cancel()
            self._recovery_timer = None
            LOGGER.info("graft_recovery_cancelled")

    def _emit_recovery_summary(self) -> None:
        self._recovery_timer = None
        try:
            counts = self._session.mailbox_work_counts()
        except Exception:
            # Recovery is a best-effort bounded wake-up, never a retry loop.
            LOGGER.exception("graft_recovery_counts_failed")
            return
        notice = MailboxRecoveryNotice(counts.unread, counts.pending_ack)
        LOGGER.info(
            "graft_recovery_counts unread=%s pending_ack=%s",
            notice.unread,
            notice.pending_ack,
        )
        if notice.unread or notice.pending_ack:
            if self._recovery_hook is None:
                LOGGER.error("graft_recovery_hook_missing")
                return
            self._recovery_hook(notice)
            LOGGER.info("graft_recovery_summary_emitted")
        else:
            LOGGER.info("graft_recovery_check_empty")

    def _deliver_nudge(self, nudge: atm_graft.PyNudge) -> None:
        message_id = nudge.message_id
        if message_id in self._recent_message_ids:
            return

        try:
            self._deliver_to_host(nudge)
        except Exception:
            raise
        self._recent_message_ids[message_id] = None
        while len(self._recent_message_ids) > self._recent_message_limit:
            self._recent_message_ids.popitem(last=False)
