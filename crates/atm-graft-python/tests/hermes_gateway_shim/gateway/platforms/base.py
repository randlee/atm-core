"""Minimal, dependency-free subset of Hermes' BasePlatformAdapter contract.

This shim is intentionally limited to the constructor, lifecycle hooks, and
value types consumed by ``atm_graft_hermes_adapter``.  The test runner uses a
real Hermes checkout instead when ``HERMES_SRC`` is supplied.
"""

from dataclasses import dataclass
from enum import Enum
from typing import Any, Awaitable, Callable, Optional


class MessageType(Enum):
    TEXT = "text"


@dataclass
class MessageEvent:
    text: str
    source: Any = None
    message_type: MessageType = MessageType.TEXT
    internal: bool = False


@dataclass
class SendResult:
    success: bool
    message_id: Optional[str] = None
    error: Optional[str] = None
    raw_response: Any = None
    retryable: bool = False


class BasePlatformAdapter:
    """Contract surface mirrored from Hermes' gateway base adapter."""

    def __init__(self, config: Any, platform: Any) -> None:
        self.config = config
        self.platform = platform
        self._message_handler: Optional[Callable[..., Awaitable[None]]] = None
        self._fatal_error_handler: Optional[Callable[..., Awaitable[None]]] = None
        self._session_store: Any = None
        self._busy_session_handler: Optional[Callable[..., Awaitable[bool]]] = None
        self._topic_recovery_fn: Optional[Callable[..., Any]] = None
        self._busy_text_mode = "interrupt"
        self._running = False

    def set_message_handler(self, handler) -> None:
        self._message_handler = handler

    def set_fatal_error_handler(self, handler) -> None:
        self._fatal_error_handler = handler

    def set_session_store(self, store) -> None:
        self._session_store = store

    def set_busy_session_handler(self, handler) -> None:
        self._busy_session_handler = handler

    def set_topic_recovery_fn(self, handler) -> None:
        self._topic_recovery_fn = handler

    def _mark_connected(self) -> None:
        self._running = True

    def _mark_disconnected(self) -> None:
        self._running = False

    def _set_fatal_error(self, _code: str, _message: str, retryable: bool = True) -> None:
        self._fatal_error_retryable = retryable

    async def _notify_fatal_error(self) -> None:
        if self._fatal_error_handler:
            result = self._fatal_error_handler(self)
            if result is not None:
                await result

