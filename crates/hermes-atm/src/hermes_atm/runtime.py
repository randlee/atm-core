"""Minimal installed Hermes/ATM runtime for the AL16 Telegram-session MVP."""

from __future__ import annotations

import asyncio
from collections.abc import Mapping
from dataclasses import dataclass
import os
from typing import Any, Callable

import atm_graft


class HermesAtmRuntimeError(RuntimeError):
    """A profile configuration or host-capability failure."""


@dataclass
class HermesAtmRuntime:
    """One profile-owned graft receiver and Telegram delivery callback."""

    session: Any
    chat_id: str
    adapter: Any
    loop: asyncio.AbstractEventLoop
    event_factory: Callable[..., Any]
    source_factory: Callable[..., Any]
    notice_text: str
    _tasks: set[asyncio.Task]

    @classmethod
    def from_gateway_runner(
        cls,
        gateway_runner: Any,
        *,
        environment: Mapping[str, str] | None = None,
        notice_text: str = "ATM nudge received; routing through your existing Telegram session.",
    ) -> "HermesAtmRuntime":
        """Compose the runtime from the host's public gateway capabilities."""

        from gateway.config import Platform
        from gateway.platforms.base import MessageEvent
        from gateway.session import SessionSource

        adapter = gateway_runner.adapters.get(Platform.TELEGRAM)
        if adapter is None:
            raise HermesAtmRuntimeError("Telegram adapter is not connected")
        loop = gateway_runner.gateway_loop or asyncio.get_running_loop()
        return cls.from_components(
            adapter=adapter,
            loop=loop,
            event_factory=MessageEvent,
            source_factory=lambda **kwargs: SessionSource(
                platform=Platform.TELEGRAM,
                **{key: value for key, value in kwargs.items() if key != "platform"},
            ),
            environment=environment,
            notice_text=notice_text,
        )

    @classmethod
    def from_components(
        cls,
        *,
        adapter: Any,
        loop: asyncio.AbstractEventLoop,
        event_factory: Callable[..., Any],
        source_factory: Callable[..., Any],
        environment: Mapping[str, str] | None = None,
        notice_text: str = "ATM nudge received; routing through your existing Telegram session.",
    ) -> "HermesAtmRuntime":
        """Compose the runtime without importing a Hermes checkout."""

        env = os.environ if environment is None else environment
        values: dict[str, str] = {}
        for name in ("ATM_HOME", "ATM_IDENTITY", "ATM_TEAM", "ATM_CHAT_ID"):
            value = env.get(name, "").strip()
            if not value:
                raise HermesAtmRuntimeError(f"{name} is required for Hermes ATM startup")
            values[name] = value

        caller = atm_graft.PyAgentAddress(
            values["ATM_IDENTITY"], values["ATM_TEAM"], values["ATM_CHAT_ID"]
        )
        # The receiver options require the profile's workspace root (where
        # ``.atm.toml`` and the roster-resolved endpoint live), not the
        # daemon home directory.  Keep ATM_HOME as the fallback for hosts
        # that intentionally co-locate those roots.
        workspace_root = env.get("ATM_WORKSPACE_ROOT", "").strip() or values["ATM_HOME"]
        options = atm_graft.PyGraftSessionOptions(
            workspace_root, values["ATM_IDENTITY"], values["ATM_TEAM"]
        )
        session = atm_graft.PyGraftSession(caller)
        runtime = cls(
            session=session,
            chat_id=values["ATM_CHAT_ID"],
            adapter=adapter,
            loop=loop,
            event_factory=event_factory,
            source_factory=source_factory,
            notice_text=notice_text,
            _tasks=set(),
        )
        session.activate_receiver(options, runtime._on_nudge)
        return runtime

    def _on_nudge(self, nudge: Any) -> None:
        """Enqueue one nudge from the native receiver callback thread."""

        def create_task() -> None:
            task = self.loop.create_task(self._enqueue_nudge(nudge))
            self._tasks.add(task)
            task.add_done_callback(self._tasks.discard)

        self.loop.call_soon_threadsafe(create_task)

    async def _enqueue_nudge(self, nudge: Any) -> None:
        body = str(nudge.body).strip()
        if not body:
            raise HermesAtmRuntimeError("ATM nudge body must not be blank")
        source = self.source_factory(
            platform="telegram",
            chat_id=self.chat_id,
            chat_type="dm",
            user_id=self.chat_id,
            profile="skillrx",
        )
        await self.adapter.send(chat_id=self.chat_id, content=self.notice_text)
        # This MVP currently uses Hermes' existing internal-event queue seam.
        # ATM steer is not implemented in this MVP and remains a planned
        # future delivery mode. This path does not call steer. ``internal=True``
        # enqueues behind an active Telegram run through Hermes' normal
        # per-session queue; it does not interrupt the current run.
        await self.adapter.handle_message(
            self.event_factory(text=body, source=source, internal=True)
        )

    def snapshot(self) -> Any:
        """Return the public receiver snapshot."""

        return self.session.snapshot()

    def close(self) -> None:
        """Close the receiver and cancel only in-flight local callbacks."""

        for task in tuple(self._tasks):
            task.cancel()
        self._tasks.clear()
        self.session.close()
