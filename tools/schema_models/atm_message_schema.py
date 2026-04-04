"""Pydantic models for docs/atm-message-schema.md."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict

from .claude_code_message_schema import ClaudeCodeInboxMessage


class AtmInboxMessage(ClaudeCodeInboxMessage):
    """Persisted inbox superset used by ATM.

    This extends the Claude Code-native shape additively. It must not be used
    to redefine the native Claude schema.
    """

    model_config = ConfigDict(extra="allow")

    source_team: str | None = None
    message_id: str | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: str | None = None
    taskId: str | None = None


class AtmMetadataFields(BaseModel):
    """Forward ATM-owned metadata namespace."""

    model_config = ConfigDict(extra="allow")

    messageId: str | None = None
    sourceTeam: str | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: str | None = None
    alertKind: str | None = None


class MessageMetadata(BaseModel):
    """Top-level metadata object with an ATM-owned namespace."""

    model_config = ConfigDict(extra="allow")

    atm: AtmMetadataFields | None = None


class AtmMetadataEnvelope(ClaudeCodeInboxMessage):
    """Forward ATM message shape using metadata.atm instead of top-level ATM fields."""

    model_config = ConfigDict(extra="allow")

    metadata: MessageMetadata | None = None


class AtmMissingTeamConfigAlertMessage(AtmInboxMessage):
    """Current ATM-authored back-channel alert notice."""

    atmAlertKind: Literal["missing_team_config"]
    missingConfigPath: str
