"""Lifecycle-owned production composition seam for the Hermes graft bridge.

The Hermes checkout owns authentication, gateway registration, and the event
loop.  It supplies those capabilities as the authenticated RPC request and
registration-backed runtime-session resolver below.  This module owns the
composition that was previously left to an undocumented external runner:
it validates the ATM profile environment, constructs the typed graft caller,
steer adapter, and bridge, then starts and closes them as one unit.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass
import os

import atm_graft
from atm_graft_hermes_adapter import (
    AtmGraftAdapter,
    HermesRequest,
    HermesSessionResolver,
    HermesSteerFailure,
    HermesRpcSteerPort,
)
from atm_graft_hermes_bridge import HermesGraftBridge


Environment = Mapping[str, str]
SessionFactory = Callable[[atm_graft.PyAgentAddress], object]


@dataclass
class HermesGraftRuntime:
    """One profile's complete bridge/adapter lifecycle.

    ``request`` and ``resolve_session_id`` must be supplied by the authenticated
    Hermes gateway.  The loader never invents transport credentials or a
    runtime session ID; it only composes the host-provided capabilities with
    the ATM graft binding.
    """

    adapter: AtmGraftAdapter
    bridge: HermesGraftBridge

    @classmethod
    def from_environment(
        cls,
        *,
        request: HermesRequest,
        resolve_session_id: HermesSessionResolver,
        environment: Environment | None = None,
        failure_hook: Callable[[HermesSteerFailure], None] | None = None,
        loop: object | None = None,
        session_factory: SessionFactory | None = None,
    ) -> "HermesGraftRuntime":
        """Compose a profile from ``ATM_*`` identity and host-owned RPC hooks."""

        env = os.environ if environment is None else environment
        values: dict[str, str] = {}
        for name in ("ATM_HOME", "ATM_IDENTITY", "ATM_TEAM", "ATM_CHAT_ID"):
            value = env.get(name, "").strip()
            if not value:
                raise ValueError(f"{name} is required for Hermes graft startup")
            values[name] = value

        caller = atm_graft.PyAgentAddress(
            values["ATM_IDENTITY"], values["ATM_TEAM"], values["ATM_CHAT_ID"]
        )
        options = atm_graft.PyGraftSessionOptions(
            values["ATM_HOME"], values["ATM_IDENTITY"], values["ATM_TEAM"]
        )
        adapter = AtmGraftAdapter(
            chat_id=values["ATM_CHAT_ID"],
            steer_port=HermesRpcSteerPort(request, resolve_session_id),
            failure_hook=failure_hook,
        )
        session = session_factory(caller) if session_factory is not None else None
        bridge = HermesGraftBridge(
            caller,
            options,
            adapter.live_nudge_callback,
            recovery_hook=adapter.recovery_summary_callback,
            loop=loop,
            session=session,
        )
        return cls(adapter=adapter, bridge=bridge)

    async def start(self) -> atm_graft.PyGraftSessionSnapshot:
        """Connect the adapter, activate the receiver, and return its state."""

        await self.adapter.connect()
        try:
            self.bridge.start()
            return self.bridge.snapshot()
        except Exception:
            self.bridge.close()
            raise

    def close(self) -> None:
        """Close the recovery timer, receiver, and daemon client exactly once."""

        self.bridge.close()
