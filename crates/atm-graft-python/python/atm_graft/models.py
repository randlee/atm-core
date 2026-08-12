"""Strict JSON ingress models shared by ATM Python tool adapters."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field


class _ToolRequest(BaseModel):
    """Reject unknown agent arguments before native transport."""

    model_config = ConfigDict(extra="forbid", strict=True)


class AtmSendRequest(_ToolRequest):
    to: str = Field(
        min_length=1,
        description=(
            "Recipient ATM address. Use a bare identity for an agent on your current team "
            "(for example, 'reviewer') or agent@team for another team "
            "(for example, 'reviewer@release'). Required."
        ),
    )
    body: str = Field(
        min_length=1,
        description="Non-empty message body to deliver to the recipient mailbox. Required.",
    )
    requires_ack: bool = Field(
        default=False,
        description="Set true when the recipient must acknowledge the message; defaults to false.",
    )


class AtmReadRequest(_ToolRequest):
    selection: Literal["actionable", "all", "unread", "pending_ack"] = Field(
        default="actionable",
        description=(
            "Mailbox bucket to search: 'actionable' (default, unread or awaiting your "
            "acknowledgement), 'all' (every visible message), 'unread' (not yet read), or "
            "'pending_ack' (messages that require your acknowledgement)."
        ),
    )
    message_id: str | None = Field(
        default=None,
        description="Exact ATM message identifier to read; omit to let the selection and filters choose a message.",
    )
    task: str | None = Field(
        default=None,
        description="Exact task identifier filter; omit to include messages from every task.",
    )
    contains: str | None = Field(
        default=None,
        description="Case-insensitive text filter for message content; omit for no text filter.",
    )
    since: str | None = Field(
        default=None,
        description="Inclusive ISO-8601 timestamp filter; omit to include messages of every age.",
    )
    from_agent: str | None = Field(
        default=None,
        description="Sender identity or agent@team filter; omit to include every sender.",
    )


class AtmListRequest(_ToolRequest):
    selection: Literal["actionable", "all", "unread", "pending_ack"] = Field(
        default="actionable",
        description=(
            "Mailbox bucket to list: 'actionable' (default, unread or awaiting your "
            "acknowledgement), 'all' (every visible message), 'unread' (not yet read), or "
            "'pending_ack' (messages that require your acknowledgement)."
        ),
    )
    limit: int | None = Field(
        default=None,
        ge=1,
        le=10_000,
        description="Maximum number of metadata rows to return, from 1 through 10000; omit for the ATM default.",
    )
    task: str | None = Field(
        default=None,
        description="Exact task identifier filter; omit to include rows from every task.",
    )
    contains: str | None = Field(
        default=None,
        description="Case-insensitive text filter for message content; omit for no text filter.",
    )
    since: str | None = Field(
        default=None,
        description="Inclusive ISO-8601 timestamp filter; omit to include rows of every age.",
    )
    from_agent: str | None = Field(
        default=None,
        description="Sender identity or agent@team filter; omit to include every sender.",
    )
