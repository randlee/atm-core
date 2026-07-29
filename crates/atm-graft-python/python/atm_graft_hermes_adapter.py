"""Hermes safe-boundary adapter for canonical ATM graft nudges.

The supported Hermes seam is the ``session.steer`` RPC method.  It accepts
``{"session_id": <configured chat id>, "text": <non-empty text>}`` and
returns a result whose status is ``queued`` or ``rejected``.  A successful
steer becomes visible after the running agent's next safe tool boundary; it is
not a normal inbound user message and it does not interrupt the active task.

This module deliberately owns no Hermes gateway transport.  A host supplies a
small async request callable through :class:`HermesRpcSteerPort`, which keeps
the ATM reference implementation independent of an external Hermes checkout.
"""

from __future__ import annotations

from collections import OrderedDict
import asyncio
from collections.abc import Awaitable, Callable, Coroutine, Mapping
from dataclasses import dataclass
import logging
from typing import Any, Protocol, runtime_checkable


LOGGER = logging.getLogger(__name__)


@runtime_checkable
class HermesSteerPort(Protocol):
    """Minimal non-interrupting Hermes session steer boundary."""

    async def steer(self, *, chat_id: str, text: str) -> None:
        """Queue ``text`` for the configured session's next safe boundary."""


class RecoveryNotice(Protocol):
    """AI.37's count-only notice shape, kept host-neutral by the bridge."""

    def render(self) -> str:
        """Render the concise durable-mail summary."""


class LiveNudge(Protocol):
    """The typed graft nudge shape consumed by this host adapter."""

    message_id: object
    source: object
    body: object


@dataclass(frozen=True)
class HermesSteerFailure(RuntimeError):
    """Visible, structured failure from the Hermes steer boundary."""

    code: str
    chat_id: str
    detail: str

    def __str__(self) -> str:
        return f"Hermes steer failed [{self.code}] for chat {self.chat_id}: {self.detail}"


HermesRequest = Callable[[str, Mapping[str, str]], Awaitable[Mapping[str, Any]]]


class HermesRpcSteerPort:
    """Adapter for Hermes' documented ``session.steer`` RPC result contract."""

    def __init__(self, request: HermesRequest) -> None:
        self._request = request

    async def steer(self, *, chat_id: str, text: str) -> None:
        if not chat_id.strip():
            raise HermesSteerFailure("invalid_chat_id", chat_id, "session_id is blank")
        if not text.strip():
            raise HermesSteerFailure("invalid_text", chat_id, "steer text is blank")
        response = await self._request(
            "session.steer", {"session_id": chat_id, "text": text}
        )
        error = response.get("error")
        if isinstance(error, Mapping):
            raise HermesSteerFailure(
                str(error.get("code", "rpc_error")),
                chat_id,
                str(error.get("message", "Hermes rejected steer")),
            )
        result = response.get("result")
        if not isinstance(result, Mapping):
            raise HermesSteerFailure("invalid_response", chat_id, "missing result")
        status = result.get("status")
        if status != "queued":
            raise HermesSteerFailure("rejected", chat_id, f"status={status!r}")


@dataclass(frozen=True)
class HermesNudgeAttribution:
    """Source retained for observability; it never selects a Hermes session."""

    message_id: str
    source: str


class AtmGraftAdapter:
    """Bind one ATM profile receiver to one configured Hermes steer session."""

    def __init__(
        self,
        *,
        chat_id: str,
        steer_port: HermesSteerPort,
        attribution_limit: int = 1_024,
    ) -> None:
        if not chat_id.strip():
            raise ValueError("ATM_CHAT_ID is required for Hermes steer delivery")
        if attribution_limit < 1:
            raise ValueError("attribution_limit must be positive")
        self._chat_id = chat_id
        self._steer_port = steer_port
        self._attribution_limit = attribution_limit
        self._attribution: OrderedDict[str, HermesNudgeAttribution] = OrderedDict()
        self._loop: asyncio.AbstractEventLoop | None = None
        self.last_failure: HermesSteerFailure | None = None

    async def connect(self) -> None:
        """Validate the single configured profile binding once per connection."""

        if not self._chat_id.strip():  # defensive against embedding mutation
            raise HermesSteerFailure("invalid_chat_id", self._chat_id, "ATM_CHAT_ID is blank")
        self._loop = asyncio.get_running_loop()
        LOGGER.info("hermes_steer_profile_connected chat_id=%s", self._chat_id)

    def live_nudge_callback(self, nudge: LiveNudge) -> None:
        """Bridge callback that queues live delivery on the configured host loop."""

        self._schedule(self.deliver_live_nudge(nudge))

    def recovery_summary_callback(self, notice: RecoveryNotice) -> None:
        """AI.37 bridge callback that queues the count-only recovery steer."""

        self._schedule(self.deliver_recovery_summary(notice))

    async def deliver_live_nudge(self, nudge: LiveNudge) -> None:
        """Deliver one typed nudge without creating a normal Hermes message."""

        message_id = str(nudge.message_id)
        self._remember_attribution(message_id, str(nudge.source))
        await self._inject_steer(str(nudge.body))

    async def deliver_recovery_summary(self, notice: RecoveryNotice) -> None:
        """Deliver AI.37's count-only recovery wake-up through the same port."""

        await self._inject_steer(notice.render())

    async def _inject_steer(self, text: str) -> None:
        """Perform the only Hermes host action allowed to this adapter."""

        try:
            if not text.strip():
                raise HermesSteerFailure("invalid_text", self._chat_id, "steer text is blank")
            await self._steer_port.steer(chat_id=self._chat_id, text=text)
        except HermesSteerFailure as failure:
            self.last_failure = failure
            LOGGER.error("hermes_steer_delivery_failed code=%s chat_id=%s", failure.code, failure.chat_id)
            raise
        except Exception as exc:
            failure = HermesSteerFailure("steer_error", self._chat_id, str(exc))
            self.last_failure = failure
            LOGGER.error("hermes_steer_delivery_failed code=%s chat_id=%s", failure.code, failure.chat_id)
            raise failure from exc

    def attribution_for(self, message_id: str) -> HermesNudgeAttribution | None:
        """Return source attribution without exposing a second session registry."""

        return self._attribution.get(message_id)

    def _remember_attribution(self, message_id: str, source: str) -> None:
        self._attribution[message_id] = HermesNudgeAttribution(message_id, source)
        self._attribution.move_to_end(message_id)
        while len(self._attribution) > self._attribution_limit:
            self._attribution.popitem(last=False)

    def _schedule(self, delivery: Coroutine[Any, Any, None]) -> None:
        loop = self._loop
        if loop is None:
            delivery.close()
            raise HermesSteerFailure("not_connected", self._chat_id, "connect before bridge delivery")

        def create_task() -> None:
            task = loop.create_task(delivery)
            task.add_done_callback(self._report_scheduled_failure)

        try:
            loop.call_soon_threadsafe(create_task)
        except RuntimeError as exc:
            delivery.close()
            raise HermesSteerFailure("loop_unavailable", self._chat_id, str(exc)) from exc

    def _report_scheduled_failure(self, task: asyncio.Task[None]) -> None:
        try:
            task.result()
        except HermesSteerFailure as failure:
            LOGGER.error("hermes_steer_delivery_failed code=%s chat_id=%s", failure.code, failure.chat_id)
        except Exception:
            LOGGER.exception("hermes_steer_delivery_failed code=unexpected chat_id=%s", self._chat_id)
