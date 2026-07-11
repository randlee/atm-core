"""Pydantic models for docs/atm-message-schema.md."""

from __future__ import annotations

from typing import Annotated, Literal

from pydantic import ConfigDict, StringConstraints

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

    message_id: UlidString | None = None
    parentMessageId: UlidString | None = None
    threadMode: Literal["add-details", "supersede"] | None = None
    taskId: str | None = None


class AtmMissingTeamConfigAlertMessage(AtmInboxMessage):
    """Current ATM-authored back-channel alert notice."""

    atmAlertKind: Literal["missing_team_config"]
    missingConfigPath: str
