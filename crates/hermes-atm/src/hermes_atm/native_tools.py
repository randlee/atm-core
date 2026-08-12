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


def _render_for_hermes(envelope: ToolEnvelope) -> str:
    """Serialize the typed ATM union at Hermes' string-only tool boundary.

    Hermes' public ``PluginContext.register_tool`` accepts structured JSON
    schemas, but its normal tool execution pipeline accepts string results
    (the only structured exception is Hermes' own multimodal envelope).  Keep
    the discriminator and detailed error payload intact as JSON text instead
    of leaking a Python ``dict`` into that host boundary.
    """

    return json.dumps(envelope, sort_keys=True, separators=(",", ":"))


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


def _invoke(call: Callable[[], Any], project: Callable[[Any], dict[str, Any]]) -> ToolEnvelope:
    try:
        # The typed graft result is projected once at this Hermes-only JSON
        # boundary; raw JSON never crosses into or out of atm_graft.
        outcome = call()
    except Exception as error:  # native binding establishes the canonical code
        return _error(
            code="ATM_NATIVE_OPERATION_FAILED",
            message=str(error),
            recovery="verify the local ATM daemon and configured identity, then retry",
            layer="native_client",
        )
    if isinstance(outcome, atm_graft.AtmToolError):
        return _native_error(outcome)
    return {"kind": "success", "result": project(outcome)}


class AtmNativeTools:
    """Per-profile handlers registered through Hermes' public plugin API."""

    def __init__(self, *, identity: str, team: str, chat_id: str) -> None:
        self._session = atm_graft.PyGraftSession(
            atm_graft.PyAgentAddress(identity, team, chat_id)
        )

    def atm_send(self, arguments: Mapping[str, Any], **_: Any) -> str:
        request = _validate(atm_graft.AtmSendRequest, arguments)
        if isinstance(request, dict):
            return _render_for_hermes(request)
        return _render_for_hermes(
            _invoke(
                lambda: self._session.send_tool(
                    request.to,
                    request.body,
                    request.requires_ack,
                    request.acknowledges_message_id,
                ),
                _send_result,
            )
        )

    def atm_read(self, arguments: Mapping[str, Any], **_: Any) -> str:
        request = _validate(atm_graft.AtmReadRequest, arguments)
        if isinstance(request, dict):
            return _render_for_hermes(request)
        return _render_for_hermes(
            _invoke(
                lambda: self._session.read_tool(
                    request.selection,
                    request.message_id,
                    request.task,
                    request.contains,
                    request.since,
                    request.from_agent,
                ),
                _read_result,
            )
        )

    def atm_list(self, arguments: Mapping[str, Any], **_: Any) -> str:
        request = _validate(atm_graft.AtmListRequest, arguments)
        if isinstance(request, dict):
            return _render_for_hermes(request)
        return _render_for_hermes(
            _invoke(
                lambda: self._session.list_tool(
                    request.selection,
                    request.limit,
                    request.task,
                    request.contains,
                    request.since,
                    request.from_agent,
                ),
                _list_result,
            )
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
        (
            "atm_send",
            tools.atm_send,
            "Send one ordinary ATM mailbox message; use the ATM CLI for administrative or advanced operations.",
        ),
        (
            "atm_read",
            tools.atm_read,
            "Read one ATM mailbox message without mutating it; use the ATM CLI for administrative or advanced operations.",
        ),
        (
            "atm_list",
            tools.atm_list,
            "List ATM mailbox metadata without mutating it; use the ATM CLI for administrative or advanced operations.",
        ),
    ):
        context.register_tool(
            name=name,
            toolset="atm",
            schema=tool_schemas()[name],
            handler=handler,
            description=description,
            emoji="📬",
        )
