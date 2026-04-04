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


class AtmMissingTeamConfigAlertMessage(AtmInboxMessage):
    """Current ATM-authored back-channel alert notice."""

    atmAlertKind: Literal["missing_team_config"]
    missingConfigPath: str
