"""Hermes native-tool adapters over the typed :mod:`atm_graft` boundary.

This module owns only tool ingress and tool envelopes.  Mailbox semantics,
transport, identity resolution, and structured outcomes stay in ``atm-graft``.
"""

from __future__ import annotations

import json
from collections.abc import Callable, Mapping
from typing import Any

import atm_graft


ToolEnvelope = dict[str, Any]


def _error(*, code: str, message: str, recovery: str, layer: str) -> ToolEnvelope:
    return {
        "kind": "error",
        "error": {
            "code": code,
            "message": message,
            "recovery": recovery,
            "layer": layer,
        },
    }


def _validate(model: type[Any], arguments: Mapping[str, Any]) -> Any | ToolEnvelope:
    try:
        return model.model_validate(dict(arguments))
    except Exception as error:
        # Pydantic's JSON projection guarantees that nested error context is
        # itself safe to return through Hermes' JSON tool protocol.
        details = json.loads(error.json()) if hasattr(error, "json") else []
        envelope = _error(
            code="invalid_request",
            message="tool arguments failed validation",
            recovery="correct the named fields and retry the tool call",
            layer="ingress_validation",
        )
        envelope["error"]["details"] = details
        return envelope


def _native_error(error: Any) -> ToolEnvelope:
    """Project the typed Rust native-client error without rebuilding it."""

    return _error(
        code=error.code,
        message=error.message,
        recovery=error.recovery,
        layer=error.layer,
    )


def _message_result(message: Any | None) -> dict[str, Any] | None:
    if message is None:
        return None
    return {
        "message_id": message.message_id,
        "source": {
            "agent": message.source.agent,
            "team": message.source.team,
            "chat_id": message.source.chat_id,
        },
        "body": message.body,
    }


def _send_result(result: Any) -> dict[str, Any]:
    return {
        "message_id": result.message_id,
        "requires_ack": result.requires_ack,
        "outcome": result.outcome,
    }


def _read_result(result: Any) -> dict[str, Any]:
    return {
        "count": result.count,
        "match_count": result.match_count,
        "additional_match_count": result.additional_match_count,
        "mutation_applied": result.mutation_applied,
        "message": _message_result(result.message),
    }


def _list_result(result: Any) -> dict[str, Any]:
    return {
        "count": result.count,
        "rows": [
            {
                "message_id": row.message_id,
                "summary": row.summary,
                "from_agent": row.from_agent,
                "timestamp": row.timestamp,
                "read": row.read,
                "pending_ack": row.pending_ack,
                "task_id": row.task_id,
            }
            for row in result.rows
        ],
    }


def _is_daemon_unavailable(outcome: Any) -> bool:
    return (
        isinstance(outcome, atm_graft.AtmToolError)
        and outcome.code == "ATM_DAEMON_UNAVAILABLE"
    )


def _invoke(
    call: Callable[[], Any],
    project: Callable[[Any], dict[str, Any]],
    *,
    reconnect: Callable[[], None],
    retry_after_reconnect: bool,
) -> ToolEnvelope:
    """Run one native operation and refresh a stale client after a daemon cycle.

    Read-only operations may replay once after refresh. A send does not: a
    transport error can occur after the daemon admits a write, so replaying it
    could create a duplicate delivery. The refresh makes the caller's next
    explicit send use the successor daemon connection.
    """

    # Native-tool methods project canonical Rust failures as AtmToolError
    # values rather than Python exceptions, preserving their public code.
    outcome = call()
    if not _is_daemon_unavailable(outcome):
        return _native_error(outcome) if isinstance(outcome, atm_graft.AtmToolError) else {
            "kind": "success",
            "result": project(outcome),
        }

    try:
        reconnect()
    except Exception as reconnect_error:
        envelope = _native_error(outcome)
        envelope["error"]["recovery"] = (
            "the native ATM session could not reconnect; verify the managed daemon is healthy, "
            f"then retry ({reconnect_error})"
        )
        return envelope

    if retry_after_reconnect:
        retry_outcome = call()
        return (
            _native_error(retry_outcome)
            if isinstance(retry_outcome, atm_graft.AtmToolError)
            else {"kind": "success", "result": project(retry_outcome)}
        )

    envelope = _native_error(outcome)
    envelope["error"]["recovery"] = (
        "the native ATM session was refreshed after a transient failure; retry this send once. "
        "The failed send was not replayed to avoid duplicate delivery"
    )
    return envelope


class AtmNativeTools:
    """Per-profile handlers registered through Hermes' public plugin API."""

    def __init__(self, *, identity: str, team: str, chat_id: str) -> None:
        self._session = atm_graft.PyGraftSession(
            atm_graft.PyAgentAddress(identity, team, chat_id)
        )

    def atm_send(self, arguments: Mapping[str, Any], **_: Any) -> ToolEnvelope:
        request = _validate(atm_graft.AtmSendRequest, arguments)
        if isinstance(request, dict):
            return request
        return _invoke(
            lambda: self._session.send_tool(request.to, request.body, request.requires_ack),
            _send_result,
            reconnect=self._session.reconnect,
            retry_after_reconnect=False,
        )

    def atm_read(self, arguments: Mapping[str, Any], **_: Any) -> ToolEnvelope:
        request = _validate(atm_graft.AtmReadRequest, arguments)
        if isinstance(request, dict):
            return request
        return _invoke(
            lambda: self._session.read_tool(
                request.selection,
                request.message_id,
                request.task,
                request.contains,
                request.since,
                request.from_agent,
            ),
            _read_result,
            reconnect=self._session.reconnect,
            retry_after_reconnect=True,
        )

    def atm_list(self, arguments: Mapping[str, Any], **_: Any) -> ToolEnvelope:
        request = _validate(atm_graft.AtmListRequest, arguments)
        if isinstance(request, dict):
            return request
        return _invoke(
            lambda: self._session.list_tool(
                request.selection,
                request.limit,
                request.task,
                request.contains,
                request.since,
                request.from_agent,
            ),
            _list_result,
            reconnect=self._session.reconnect,
            retry_after_reconnect=True,
        )


def tool_schemas() -> dict[str, dict[str, Any]]:
    """Return the public JSON schemas; Pydantic remains the ingress authority."""

    return {
        "atm_send": atm_graft.AtmSendRequest.model_json_schema(),
        "atm_read": atm_graft.AtmReadRequest.model_json_schema(),
        "atm_list": atm_graft.AtmListRequest.model_json_schema(),
    }


def register_tools(context: Any, *, identity: str, team: str, chat_id: str) -> None:
    """Register all ATM tools using Hermes' supported PluginContext seam."""

    tools = AtmNativeTools(identity=identity, team=team, chat_id=chat_id)
    for name, handler, description in (
        ("atm_send", tools.atm_send, "Send an ATM mailbox message."),
        ("atm_read", tools.atm_read, "Read one ATM mailbox message without mutating it."),
        ("atm_list", tools.atm_list, "List ATM mailbox metadata without mutating it."),
    ):
        context.register_tool(
            name=name,
            toolset="atm",
            schema=tool_schemas()[name],
            handler=handler,
            description=description,
            emoji="📬",
        )
