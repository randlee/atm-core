"""
Hermes gateway platform adapter for ATM graft.

Wires ATM graft nudges into the Hermes gateway as a first-class platform,
just like Telegram, Discord, etc.  Messages from other agents arrive as
native inbound messages and route through the normal gateway pipeline.

Install: pip install atm-graft  (or maturin develop)
Enable:  hermes gateway setup → enable "atm" platform
         or set platforms.atm.enabled: true in config.yaml

Configuration:
  ATM_IDENTITY  — agent name  (default: from profile)
  ATM_TEAM      — team name   (default: from profile)
  ATM_HOME      — ATM home directory (default: ~/.atm)
  ATM_CHAT_ID   — optional chat id for the caller address
"""

from __future__ import annotations

import asyncio
import logging
import os
import sys
import threading
from pathlib import Path
from typing import Any, Optional

logger = logging.getLogger(__name__)


def _hermes_types():
    """Load Hermes adapter types lazily, after plugin discovery."""
    from gateway.config import Platform
    from gateway.platforms.base import BasePlatformAdapter, SendResult

    return Platform, BasePlatformAdapter, SendResult


def _float_env(name: str, default: float) -> float:
    try:
        return float(os.environ[name])
    except (KeyError, ValueError):
        return default


# ---------------------------------------------------------------------------
# Plugin entry point
# ---------------------------------------------------------------------------

def register(ctx) -> None:
    """Register the ATM graft platform with the Hermes gateway."""
    ctx.register_platform(
        name="atm",
        label="ATM (Agent Team Mail)",
        adapter_factory=_build_adapter,
        check_fn=_check_requirements,
        required_env=[],
        install_hint="pip install atm-graft",
        emoji="📬",
        max_message_length=4096,
    )


def _check_requirements() -> bool:
    """Check that atm_graft is importable and ATM_HOME is configured."""
    try:
        import atm_graft  # noqa: F401
    except ImportError:
        logger.warning("atm_graft not installed — run: pip install atm-graft")
        return False
    atm_home = os.environ.get("ATM_HOME", os.path.expanduser("~/.atm"))
    if not Path(atm_home).exists():
        logger.warning("ATM_HOME (%s) does not exist", atm_home)
        return False
    return True


def _build_adapter(config):
    """Build an AtmGraftAdapter from the platform config."""
    return AtmGraftAdapter(config)


# ---------------------------------------------------------------------------
# Shared imports (lazy — only loaded when the adapter is actually used)
# ---------------------------------------------------------------------------

_atm_graft = None
_HermesGraftBridge = None


def _ensure_imports():
    global _atm_graft, _HermesGraftBridge
    if _atm_graft is None:
        import atm_graft as _atm_graft
    if _HermesGraftBridge is None:
        # Add the bridge source to path (it ships alongside this module)
        _here = Path(__file__).resolve().parent
        if str(_here) not in sys.path:
            sys.path.insert(0, str(_here))
        from atm_graft_hermes_bridge import HermesGraftBridge as _HermesGraftBridge


# ---------------------------------------------------------------------------
# Adapter
# ---------------------------------------------------------------------------

class AtmGraftAdapter(_hermes_types()[1]):
    """Gateway platform adapter that receives ATM grafts and injects them
    as internal MessageEvents into the Hermes message pipeline."""

    def __init__(self, config) -> None:
        Platform, _, _ = _hermes_types()
        # Hermes has no built-in ATM enum (the plugin is the platform owner),
        # so LOCAL is the neutral base value.  ``platform`` remains writable
        # because BasePlatformAdapter initialises it during super().__init__.
        super().__init__(config, Platform.LOCAL)
        self._config = config
        self._bridge = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._ready = threading.Event()

        # Agent identity
        self._agent = os.environ.get("ATM_IDENTITY", "skillrx")
        self._team = os.environ.get("ATM_TEAM", "hermes")
        self._chat_id = os.environ.get("ATM_CHAT_ID") or None
        self._atm_home = os.environ.get("ATM_HOME", os.path.expanduser("~/.atm"))

        # Graft endpoints are workspace-scoped, not Hermes-profile-scoped.
        # Require an explicit workspace when the gateway's cwd is a profile;
        # falling back to cwd keeps local development usable while making the
        # root choice visible in logs.
        workspace = (
            os.environ.get("ATM_WORKSPACE_ROOT")
            or os.environ.get("HERMES_WORKSPACE_ROOT")
            or os.getcwd()
        )
        self._workspace_root = str(Path(workspace).expanduser().resolve())

    # -- Adapter interface (called by the gateway) ---------------------------

    @property
    def platform(self):
        return self._platform

    @platform.setter
    def platform(self, value) -> None:
        self._platform = value

    @property
    def name(self) -> str:
        return "atm"

    async def connect(self, *, is_reconnect: bool = False) -> bool:
        """Start the graft bridge and activate the receiver."""
        try:
            _ensure_imports()

            os.environ.setdefault("ATM_HOME", self._atm_home)
            os.environ.setdefault("ATM_IDENTITY", self._agent)
            os.environ.setdefault("ATM_TEAM", self._team)

            self._loop = asyncio.get_running_loop()
            workspace = Path(self._workspace_root)
            if not workspace.is_dir():
                raise RuntimeError(f"ATM_WORKSPACE_ROOT is not a directory: {workspace}")

            caller = _atm_graft.PyAgentAddress(self._agent, self._team, self._chat_id)
            options = _atm_graft.PyGraftSessionOptions(
                self._workspace_root, self._agent, self._team
            )

            def _on_nudge(chat_key: str, body: str) -> None:
                """Called from the graft receiver thread."""
                if self._loop and self._message_handler:
                    asyncio.run_coroutine_threadsafe(
                        self._dispatch_nudge(chat_key, body),
                        self._loop,
                    )

            self._bridge = _HermesGraftBridge(caller, options, _on_nudge)
            self._bridge.start()
            snap = self._bridge.snapshot()

            if snap.state != "listening":
                raise RuntimeError(f"ATM graft bridge not listening: state={snap.state}")

            self._ready.set()
            self._mark_connected()
            logger.info(
                "ATM graft bridge connected: agent=%s team=%s state=%s endpoint=%s reconnect=%s",
                self._agent, self._team, snap.state,
                workspace / ".atm" / "graft" / self._team / f"{self._agent}.json",
                is_reconnect,
            )
            return True
        except Exception as exc:
            self._set_fatal_error("atm_graft_connect", str(exc), retryable=True)
            logger.exception("ATM graft bridge failed to connect")
            await self._notify_fatal_error()
            return False

    async def disconnect(self) -> None:
        """Close the graft bridge."""
        self._running = False
        if self._bridge:
            self._bridge.close()
            self._bridge = None
        self._mark_disconnected()
        logger.info("ATM graft bridge disconnected")

    async def send(
        self,
        chat_id: str,
        content: str,
        reply_to: Optional[str] = None,
        metadata: Optional[dict[str, Any]] = None,
    ):
        """Send a response back via ATM (ack/reply)."""
        _, _, SendResult = _hermes_types()
        import atm_graft as _ag
        try:
            session = _ag.PyGraftSession(
                _ag.PyAgentAddress(self._agent, self._team, self._chat_id)
            )
            target = self._target_from_chat_id(_ag, chat_id)
            if target is None:
                return SendResult(success=False, error=f"invalid ATM chat id: {chat_id}")
            try:
                session.send(target, content)
            finally:
                session.close()
            return SendResult(success=True)
        except Exception as exc:
            logger.exception("Failed to send ATM response")
            return SendResult(success=False, error=str(exc), retryable=True)

    async def get_chat_info(self, chat_id: str) -> dict[str, Any]:
        return {"name": chat_id, "type": "dm"}

    def _target_from_chat_id(self, graft, chat_id: str):
        value = str(chat_id or "").removeprefix("atm:")
        if "@" not in value:
            return None
        agent_team, team = value.rsplit("@", 1)
        if not agent_team or not team:
            return None
        if ":" in agent_team:
            agent, chat_id = agent_team.split(":", 1)
        else:
            agent, chat_id = agent_team, None
        return graft.PyAgentAddress(agent, team, chat_id or None)

    # -- Internal ------------------------------------------------------------

    async def _dispatch_nudge(self, chat_key: str, body: str) -> None:
        """Create a MessageEvent and dispatch through the gateway pipeline."""
        if not self._message_handler:
            return

        from gateway.platforms.base import MessageEvent, MessageType
        from gateway.session import SessionSource
        from gateway.config import Platform

        # Graft is an ingress transport, not a second Hermes conversation.
        # Route the synthetic event into the configured Telegram DM so it
        # shares the live Telegram session and normal Telegram egress.  The
        # ATM sender remains useful as the synthetic user identity, but must
        # not become the session/chat key (that would create an isolated
        # ``platform=local`` conversation and never wake Telegram).
        if not hasattr(Platform, "TELEGRAM"):
            logger.error(
                "ATM graft nudge dropped: Platform.TELEGRAM not found in gateway config"
            )
            return
        telegram_platform = Platform.TELEGRAM
        if not self._chat_id:
            logger.error(
                "ATM graft nudge dropped: ATM_CHAT_ID is required for Telegram routing"
            )
            return

        source = SessionSource(
            platform=telegram_platform,
            chat_id=self._chat_id,
            chat_type="dm",
            # Use the configured Telegram identity for the gateway's normal
            # authorization check.  The ATM source remains in user_name so
            # the event is still attributable without requiring a synthetic
            # user to be added to TELEGRAM_ALLOWED_USERS.
            user_id=self._chat_id,
            user_name=chat_key,
        )

        event = MessageEvent(
            text=body,
            source=source,
            message_type=MessageType.TEXT,
            internal=False,
        )

        await self._message_handler(event)
