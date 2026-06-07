"""Pydantic models for docs/legacy-atm-message-schema.md."""

from __future__ import annotations

from typing import Annotated

from pydantic import BaseModel, ConfigDict, StringConstraints

from .claude_code_message_schema import ClaudeCodeInboxMessage

from .atm_message_schema import AtmInboxMessage

UlidString = Annotated[
    str,
    StringConstraints(pattern=r"^[0-7][0-9A-HJKMNP-TV-Z]{25}$"),
]


class LegacyAtmInboxMessage(AtmInboxMessage):
    """Historical ATM-added top-level fields accepted on read only."""

    model_config = ConfigDict(extra="allow")

    source_team: str | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: UlidString | None = None
    atmAlertKind: str | None = None
    missingConfigPath: str | None = None


class LegacyAtmMetadataFields(BaseModel):
    """Historical ATM-owned metadata namespace accepted on read only."""

    model_config = ConfigDict(extra="allow")

    messageId: UlidString | None = None
    sourceTeam: str | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: UlidString | None = None
    taskId: str | None = None
    alertKind: str | None = None


class LegacyMessageMetadata(BaseModel):
    """Historical top-level metadata container accepted on read only."""

    model_config = ConfigDict(extra="allow")

    atm: LegacyAtmMetadataFields | None = None


class LegacyAtmMetadataEnvelope(ClaudeCodeInboxMessage):
    """Historical `metadata.atm` derivative accepted on read only."""

    model_config = ConfigDict(extra="allow")

    metadata: LegacyMessageMetadata | None = None
