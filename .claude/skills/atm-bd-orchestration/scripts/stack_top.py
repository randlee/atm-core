# pyright: strict
"""Select a phase's current stack top from its canonical bead ledger."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import TypeAlias, TypeGuard

JsonValue: TypeAlias = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]


def is_object_mapping(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def is_json_object(value: object) -> TypeGuard[JsonObject]:
    if not is_object_mapping(value):
        return False
    for key, item in value.items():
        if not isinstance(key, str) or not is_json_value(item):
            return False
    return True


def is_json_value(value: object) -> TypeGuard[JsonValue]:
    if value is None or isinstance(value, (bool, int, float, str)):
        return True
    if is_object_list(value):
        return all(is_json_value(item) for item in value)
    return is_json_object(value)


def is_object_list(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def stack_bead_id(root: str) -> str:
    """Return the stack ledger bead ID for a phase root."""
    return f"{root}.stack"


@dataclass(frozen=True)
class StackTop:
    pr: int | None
    branch: str | None
    head: str | None


@dataclass(frozen=True)
class StackTopMismatch:
    ledger_top: StackTop | None
    hosted_top: int | None


def select_passed_unlinked_top(layers: list[JsonValue]) -> StackTop | None:
    """Return the passed first layer while it has no hosted stack, excluding holds."""
    for layer in reversed(layers):
        if not is_json_object(layer) or layer.get("held_on") or layer.get("linked") is not False:
            continue
        pr, branch, head = layer.get("pr"), layer.get("branch"), layer.get("head")
        if type(pr) is not int or not isinstance(branch, str) or not branch.strip() or not isinstance(head, str) or not head.strip():
            raise ValueError("passed stack layer must have a PR, branch and head")
        assert isinstance(pr, int)
        return StackTop(pr, branch, head)
    return None


def select_confirmed_stack_top(
    ledger_layers: list[JsonValue], hosted_prs: list[int] | None = None,
) -> StackTop | StackTopMismatch:
    """Select the last linked ledger layer and compare it with hosted membership."""
    linked: list[StackTop] = []
    for layer in ledger_layers:
        if not is_json_object(layer):
            raise ValueError("stack layer must be an object")
        if layer.get("linked") is not True:
            continue
        pr, branch, head = layer.get("pr"), layer.get("branch"), layer.get("head")
        if type(pr) is not int or not isinstance(branch, str) or not branch.strip() or not isinstance(head, str) or not head.strip():
            raise ValueError("linked stack layer must have a PR, branch and head")
        assert isinstance(pr, int)
        linked.append(StackTop(pr, branch, head))
    ledger_top = linked[-1] if linked else None
    if hosted_prs is None:
        return ledger_top or StackTop(None, None, None)
    hosted_top = hosted_prs[-1] if hosted_prs else None
    if (ledger_top.pr if ledger_top else None) != hosted_top:
        return StackTopMismatch(ledger_top, hosted_top)
    return ledger_top or StackTop(None, None, None)


def select_stack_top(
    bead: JsonObject,
    pr_target: object,
    resolve_head: Callable[[str], object],
) -> tuple[str, str]:
    """Compatibility wrapper for callers without hosted membership."""
    metadata = bead.get("metadata")
    if metadata is not None and not is_json_object(metadata):
        raise ValueError("stack bead metadata must be an object or null")
    layers = metadata.get("layers", []) if metadata is not None else []
    if not isinstance(layers, list):
        raise ValueError("stack bead metadata.layers must be an array")
    if layers:
        last = layers[-1]
        if not is_json_object(last):
            raise ValueError("last stack layer must have non-empty branch and head strings")
        branch, head = last.get("branch"), last.get("head")
        if not isinstance(branch, str) or not branch.strip() or not isinstance(head, str) or not head.strip():
            raise ValueError("last stack layer must have non-empty branch and head strings")
    selected = select_confirmed_stack_top(layers)
    if isinstance(selected, StackTop) and selected.branch is not None and selected.head is not None:
        return selected.branch, selected.head
    passed = select_passed_unlinked_top(layers)
    if passed is not None:
        assert passed.branch is not None and passed.head is not None
        return passed.branch, passed.head
    if not isinstance(pr_target, str) or not pr_target.strip():
        raise ValueError("pr_target must be a non-empty string")
    head = resolve_head(pr_target)
    if not isinstance(head, str) or not head.strip():
        raise ValueError(f"origin/{pr_target} has no resolvable head")
    return pr_target, head
