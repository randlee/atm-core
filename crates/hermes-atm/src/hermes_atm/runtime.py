"""Minimal installed Hermes/ATM runtime for the AL16 Telegram-session MVP."""

from __future__ import annotations

import asyncio
from collections.abc import Mapping
from dataclasses import dataclass
import os
from typing import Any, Awaitable, Callable

import atm_graft

class HermesAtmRuntimeError(RuntimeError):
    """A profile configuration or host-capability failure."""


@dataclass
class HermesAtmRuntime:
    """One profile-owned graft receiver and Telegram delivery callback."""

    session: Any
    chat_id: str
    profile: str
    loop: asyncio.AbstractEventLoop
    inject_internal_message: Callable[..., Awaitable[Any]]
    platform: Any
    notice_text: str | None
    _tasks: set[asyncio.Task]

    @classmethod
    def from_gateway_runner(
        cls,
        gateway_runner: Any,
        *,
        profile: str,
        environment: Mapping[str, str] | None = None,
        notice_text: str | None = None,
    ) -> "HermesAtmRuntime":
        """Compose the runtime from the host's public gateway capabilities."""

        from gateway.config import Platform

        injector = getattr(gateway_runner, "inject_internal_message", None)
        if not callable(injector):
            raise HermesAtmRuntimeError(
                "Hermes gateway does not expose inject_internal_message"
            )
        # Startup hooks run on the gateway event loop, but some compatible
        # host revisions publish the runner before exposing gateway_loop.
        # Use that active loop as the portable fallback so receiver
        # publication does not fail solely on optional host bookkeeping.
        loop = getattr(gateway_runner, "gateway_loop", None) or asyncio.get_running_loop()
        return cls.from_components(
            inject_internal_message=injector,
            loop=loop,
            platform=Platform.TELEGRAM,
            profile=profile,
            environment=environment,
            notice_text=notice_text,
        )

    @classmethod
    def from_components(
        cls,
        *,
        inject_internal_message: Callable[..., Awaitable[Any]],
        loop: asyncio.AbstractEventLoop,
        platform: Any,
        profile: str,
        environment: Mapping[str, str] | None = None,
        notice_text: str | None = None,
    ) -> "HermesAtmRuntime":
        """Compose the runtime without importing a Hermes checkout."""

        env = os.environ if environment is None else environment
        profile = profile.strip()
        if not profile:
            raise HermesAtmRuntimeError("Hermes profile is required for startup")
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
            profile=profile,
            loop=loop,
            inject_internal_message=inject_internal_message,
            platform=platform,
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
        # Hermes owns session identity and busy-session queueing behind this
        # public runner seam. The graft receiver supplies only the explicit
        # profile, real Telegram platform, configured chat id, and body.
        #
        # The body is the canonical `<atm …>` nudge for the agent loop. Its
        # separately rendered plain-text notice gives the Telegram user the
        # sender and topic without presenting dispatch XML as user-visible
        # text. A host may deliberately supply a meaningful override.
        visible_notice = str(getattr(nudge, "notice_text", "")).strip() or body
        await self.inject_internal_message(
            profile=self.profile,
            platform=self.platform,
            chat_id=self.chat_id,
            text=body,
            mode="queue",
            notice_text=visible_notice if self.notice_text is None else self.notice_text,
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
