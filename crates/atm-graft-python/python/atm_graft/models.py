"""Strict JSON ingress models shared by ATM Python tool adapters."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


class _ToolRequest(BaseModel):
    """Reject unknown agent arguments before native transport."""

    model_config = ConfigDict(extra="forbid", strict=True)


class AtmSendRequest(_ToolRequest):
    to: str = Field(min_length=1)
    body: str = Field(min_length=1)
    requires_ack: bool = False


class AtmReadRequest(_ToolRequest):
    selection: Literal["actionable", "all", "unread", "pending_ack"] = "actionable"
    message_id: str | None = None
    task: str | None = None
    contains: str | None = None
    since: str | None = None
    from_agent: str | None = None


class AtmListRequest(_ToolRequest):
    selection: Literal["actionable", "all", "unread", "pending_ack"] = "actionable"
    limit: int | None = Field(default=None, ge=1, le=10_000)
    task: str | None = None
    contains: str | None = None
    since: str | None = None
    from_agent: str | None = None
