"""Discriminated-union results for plan-anchoring scripts (Python 3.9 floor).

A function that can fail in an expected way returns ``Result[T, E]``: either
``Ok(value)`` or ``Err(error)``. Callers branch on ``isinstance(r, Err)`` (or
the ``ok`` literal) instead of catching exceptions. Exceptions are reserved for
bugs; a CLI converts an unexpected one to ``SCRIPT.INTERNAL`` at its boundary.

Standard library only.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Generic, Literal, Mapping, Optional, TypeVar, Union

T = TypeVar("T")
E = TypeVar("E")


@dataclass(frozen=True)
class Ok(Generic[T]):
    value: T
    ok: Literal[True] = field(default=True, init=False)


@dataclass(frozen=True)
class Err(Generic[E]):
    error: E
    ok: Literal[False] = field(default=False, init=False)


Result = Union[Ok[T], Err[E]]


@dataclass(frozen=True)
class ToolError:
    """An expected failure: a stable ``code``, a ``message``, and the ``action`` that fixes it."""

    code: str
    message: str
    action: Optional[str] = None
    data: Optional[Mapping[str, Any]] = None

    def to_json(self) -> dict:
        return {
            "code": self.code,
            "message": self.message,
            "recoverable": False,
            "suggested_action": self.action,
        }


def err(code: str, message: str, action: Optional[str] = None,
        data: Optional[Mapping[str, Any]] = None) -> Err[ToolError]:
    return Err(ToolError(code=code, message=message, action=action, data=data))
