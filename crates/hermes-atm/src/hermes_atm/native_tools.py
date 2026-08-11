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


def _native_error(error: Exception) -> ToolEnvelope:
    code = str(getattr(error, "code", "native_operation_failed"))
    message = str(getattr(error, "message", str(error)))
    return _error(
        code=code,
        message=message,
        recovery="verify the local ATM daemon and configured identity, then retry",
        layer="native_client",
    )


def _invoke(call: Callable[[], str]) -> ToolEnvelope:
    try:
        return {"kind": "success", "result": json.loads(call())}
    except Exception as error:  # native binding establishes the canonical code
        return _native_error(error)


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
            lambda: self._session.send_tool_json(
                request.to, request.body, request.requires_ack
            )
        )

    def atm_read(self, arguments: Mapping[str, Any], **_: Any) -> ToolEnvelope:
        request = _validate(atm_graft.AtmReadRequest, arguments)
        if isinstance(request, dict):
            return request
        return _invoke(
            lambda: self._session.read_tool_json(
                request.selection,
                request.message_id,
                request.task,
                request.contains,
                request.since,
                request.from_agent,
            )
        )

    def atm_list(self, arguments: Mapping[str, Any], **_: Any) -> ToolEnvelope:
        request = _validate(atm_graft.AtmListRequest, arguments)
        if isinstance(request, dict):
            return request
        return _invoke(
            lambda: self._session.list_tool_json(
                request.selection,
                request.limit,
                request.task,
                request.contains,
                request.since,
                request.from_agent,
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
