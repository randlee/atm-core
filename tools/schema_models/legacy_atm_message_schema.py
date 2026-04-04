"""Pydantic models for docs/legacy-atm-message-schema.md."""

from __future__ import annotations

from pydantic import ConfigDict

from .atm_message_schema import AtmInboxMessage


class LegacyAtmInboxMessage(AtmInboxMessage):
    """Historical ATM-added top-level fields accepted on read only."""

    model_config = ConfigDict(extra="allow")

    atmAlertKind: str | None = None
    missingConfigPath: str | None = None
