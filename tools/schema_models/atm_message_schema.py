"""Pydantic models for docs/atm-message-schema.md."""

from __future__ import annotations

from typing import Annotated, Literal
from uuid import UUID

from pydantic import BaseModel, ConfigDict, StringConstraints, model_validator

from .claude_code_message_schema import ClaudeCodeInboxMessage

UlidString = Annotated[
    str,
    StringConstraints(pattern=r"^[0-7][0-9A-HJKMNP-TV-Z]{25}$"),
]


class AtmInboxMessage(ClaudeCodeInboxMessage):
    """Persisted inbox superset used by ATM.

    This extends the Claude Code-native shape additively. It must not be used
    to redefine the native Claude schema.
    """

    model_config = ConfigDict(extra="allow")

    source_team: str | None = None
    message_id: UUID | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: UUID | None = None
    taskId: str | None = None


class AtmMetadataFields(BaseModel):
    """Forward ATM-owned metadata namespace."""

    model_config = ConfigDict(extra="allow")

    messageId: UlidString | None = None
    sourceTeam: str | None = None
    pendingAckAt: str | None = None
    acknowledgedAt: str | None = None
    acknowledgesMessageId: UlidString | None = None
    taskId: str | None = None
    alertKind: str | None = None


class MessageMetadata(BaseModel):
    """Top-level metadata object with an ATM-owned namespace."""

    model_config = ConfigDict(extra="allow")

    atm: AtmMetadataFields | None = None


class AtmMetadataEnvelope(ClaudeCodeInboxMessage):
    """Forward ATM message shape using metadata.atm instead of top-level ATM fields."""

    model_config = ConfigDict(extra="allow")

    metadata: MessageMetadata | None = None

    @model_validator(mode="before")
    @classmethod
    def reject_top_level_atm_machine_fields(cls, value: object) -> object:
        if not isinstance(value, dict):
            return value

        forbidden = {
            "message_id",
            "source_team",
            "pendingAckAt",
            "acknowledgedAt",
            "acknowledgesMessageId",
            "taskId",
        }
        leaked = sorted(forbidden.intersection(value.keys()))
        if leaked:
            raise ValueError(
                "ATM machine fields must not appear at top level for forward writes: "
                + ", ".join(leaked)
            )
        return value


class AtmMissingTeamConfigAlertMessage(AtmInboxMessage):
    """Current ATM-authored back-channel alert notice."""

    atmAlertKind: Literal["missing_team_config"]
    missingConfigPath: str
