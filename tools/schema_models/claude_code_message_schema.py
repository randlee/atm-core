"""Pydantic models for docs/claude-code-message-schema.md.

These models intentionally cover only the Claude Code-native message contract
documented in this repository. ATM additive fields must not be added here as if
they were native Claude schema fields.
"""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


class ClaudeCodeInboxMessage(BaseModel):
    """Claude Code-native inbox message shape consumed by ATM.

    Unknown additive fields are allowed because the documented native schema is
    additive and ATM must tolerate producer-owned extensions without claiming
    ownership of them.
    """

    model_config = ConfigDict(extra="allow", populate_by_name=True)

    from_: str = Field(alias="from")
    text: str
    timestamp: str
    read: bool
    summary: str | None = None
    color: str | None = None


class ClaudeCodeIdleNotificationText(BaseModel):
    """Claude Code-native idle-notification payload encoded inside `text`."""

    model_config = ConfigDict(extra="allow", populate_by_name=True)

    type: Literal["idle_notification"]
    from_: str = Field(alias="from")
    timestamp: str
    idleReason: str
