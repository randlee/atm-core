# pyright: strict
"""Typed records decoded from operation-specific bd, atm and gh JSON output.

The JSON transport stays generic. Callers validate records for the operation
they invoked before passing them to report and graph logic.
"""
from __future__ import annotations

import json
import inspect
import sys
from collections.abc import Mapping
from types import UnionType
from typing import Any, ForwardRef, Literal, NotRequired, Required, TypeAlias, TypeGuard, TypedDict, TypeVar, Union, cast, get_args, get_origin, get_type_hints, is_typeddict

JsonValue: TypeAlias = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]

T = TypeVar("T")


class DependencyRecord(TypedDict, total=False):
    type: str
    dependency_type: str
    depends_on_id: str
    id: str


class BeadMetadata(TypedDict, total=False):
    branch: str
    checked_bead: str
    difficulty: str
    integration_branch: str
    layer: str | int | float | bool | None
    phase: str
    round: str | int | float | bool | None
    severity: str
    stack: str
    sprint: str
    verdict: str
    worktree: str
    pr_target: str


class BeadRecord(TypedDict):
    id: Required[str]
    status: Required[str]
    title: NotRequired[str]
    close_reason: NotRequired[str]
    labels: NotRequired[list[str]]
    dependencies: NotRequired[list[DependencyRecord]]
    metadata: NotRequired[BeadMetadata]
    parent: NotRequired[str]
    priority: NotRequired[int | str]
    blocked_by: NotRequired[list[str]]


class AtmMember(TypedDict, total=False):
    name: str
    identity: str
    id: str
    agent_type: str
    model: str


class AtmTask(TypedDict):
    task_id: Required[str]
    assignee: NotRequired[str]
    owner: NotRequired[str]
    state: NotRequired[str]
    status: NotRequired[str]


class AtmTaskEvent(TypedDict, total=False):
    event: Required[str]
    at: str
    to_state: str


class PullRequestRef(TypedDict):
    ref: Required[str]


class PullRequest(TypedDict, total=False):
    number: Required[int]
    state: Required[str]
    merged_at: str | None
    mergedAt: str | None
    head: PullRequestRef
    base: PullRequestRef
    isDraft: bool
    statusCheckRollup: list["PullRequestCheck"]
    headRefName: str
    baseRefName: str
    title: str
    mergeStateStatus: str
    mergeable: str


class PullRequestCheck(TypedDict, total=False):
    conclusion: str | None
    state: str | None
    status: str | None


def is_object_mapping(value: object) -> TypeGuard[dict[object, object]]:
    return isinstance(value, dict)


def is_object_list(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def is_object(value: object) -> TypeGuard[dict[str, object]]:
    if not is_object_mapping(value):
        return False
    return all(isinstance(key, str) for key in value)


def is_json_value(value: object) -> TypeGuard[JsonValue]:
    if value is None or isinstance(value, (bool, int, float, str)):
        return True
    if is_object_list(value):
        return all(is_json_value(item) for item in value)
    if not is_object(value):
        return False
    return all(is_json_value(item) for item in value.values())


def is_json_object(value: object) -> TypeGuard[JsonObject]:
    return is_object(value) and all(is_json_value(item) for item in value.values())


def bead_record_to_json(record: BeadRecord) -> JsonObject:
    """Convert a validated bead DTO for the generic schema validator boundary."""
    decoded: object = json.loads(json.dumps(record))
    if not is_json_object(decoded):
        raise ValueError("validated bead record could not be encoded as a JSON object")
    return decoded


def _matches_annotation(value: object, annotation: object) -> bool:
    if isinstance(annotation, (str, ForwardRef)):
        annotation = eval(annotation if isinstance(annotation, str) else annotation.__forward_arg__, globals())
    if annotation is Any or annotation is object:
        return True
    if annotation is type(None):
        return value is None

    origin = get_origin(annotation)
    args = cast(tuple[object, ...], get_args(annotation))
    if origin is Literal:
        return any(type(value) is type(item) and value == item for item in args)
    if origin in (Union, UnionType):
        return any(_matches_annotation(value, item) for item in args)
    if origin is list:
        return isinstance(value, list) and all(
            _matches_annotation(item, args[0]) for item in cast(list[object], value)
        )
    if origin in (dict, Mapping):
        return isinstance(value, Mapping) and all(
            _matches_annotation(key, args[0]) and _matches_annotation(item, args[1])
            for key, item in cast(Mapping[object, object], value).items()
        )
    if isinstance(annotation, type) and is_typeddict(annotation):
        return _matches_typeddict(value, cast(type[Any], annotation))
    return type(value) is annotation if annotation is int or annotation is bool or annotation is float else (
        isinstance(value, annotation) if isinstance(annotation, type) else False
    )


def _matches_typeddict(value: object, schema: type[Any]) -> bool:
    if not isinstance(value, dict):
        return False
    mapping = cast(dict[object, object], value)
    if not all(isinstance(key, str) for key in mapping):
        return False

    module = sys.modules.get(schema.__module__)
    namespace = vars(module) if module is not None else None
    frame = inspect.currentframe()
    while namespace is None and frame is not None:
        if frame.f_globals.get("__name__") == schema.__module__:
            namespace = frame.f_globals
        frame = frame.f_back
    del frame
    hints = get_type_hints(schema, globalns=namespace or globals(), include_extras=True)
    required_keys = set(schema.__required_keys__)
    optional_keys = set(schema.__optional_keys__)
    for key, annotation in hints.items():
        origin = get_origin(annotation)
        if origin is Required:
            required_keys.add(key)
            optional_keys.discard(key)
        elif origin is NotRequired:
            optional_keys.add(key)
            required_keys.discard(key)
        if key in optional_keys and key not in value:
            continue
        if key in required_keys and key not in value:
            return False
        if key in mapping and not _matches_annotation(
            mapping[key],
            cast(tuple[object, ...], get_args(annotation))[0] if origin in (Required, NotRequired) else annotation,
        ):
            return False

    return required_keys <= value.keys()


def matches_typeddict(value: object, schema: type[T]) -> TypeGuard[T]:
    """Check a mapping against a TypedDict's annotations, preserving extra keys."""
    return _matches_typeddict(value, schema)


def is_dependency_record(value: object) -> TypeGuard[DependencyRecord]:
    if not is_object(value):
        return False
    kind = value.get("type", value.get("dependency_type"))
    target = value.get("depends_on_id", value.get("id"))
    return isinstance(kind, str) and isinstance(target, str) and matches_typeddict(value, DependencyRecord)


def is_bead_metadata(value: object) -> TypeGuard[BeadMetadata]:
    return matches_typeddict(value, BeadMetadata)


def is_bead_record(value: object) -> TypeGuard[BeadRecord]:
    return (matches_typeddict(value, BeadRecord)
            and all(is_dependency_record(edge) for edge in value.get("dependencies", [])))


def is_bead_records(value: object) -> TypeGuard[list[BeadRecord]]:
    return is_object_list(value) and all(is_bead_record(row) for row in value)


def is_atm_member(value: object) -> TypeGuard[AtmMember]:
    return (matches_typeddict(value, AtmMember)
            and any(isinstance(value.get(key), str) for key in ("name", "identity", "id")))


def is_atm_members(value: object) -> TypeGuard[list[AtmMember]]:
    return is_object_list(value) and all(is_atm_member(row) for row in value)


def is_atm_task(value: object) -> TypeGuard[AtmTask]:
    return matches_typeddict(value, AtmTask)


def is_atm_tasks(value: object) -> TypeGuard[list[AtmTask]]:
    return is_object_list(value) and all(is_atm_task(row) for row in value)


def is_atm_task_event(value: object) -> TypeGuard[AtmTaskEvent]:
    return matches_typeddict(value, AtmTaskEvent)


def is_atm_task_events(value: object) -> TypeGuard[list[AtmTaskEvent]]:
    return is_object_list(value) and all(is_atm_task_event(row) for row in value)


def is_pull_request_ref(value: object) -> TypeGuard[PullRequestRef]:
    return matches_typeddict(value, PullRequestRef)


def is_pull_request_check(value: object) -> TypeGuard[PullRequestCheck]:
    return matches_typeddict(value, PullRequestCheck)


def is_pull_request(value: object) -> TypeGuard[PullRequest]:
    return matches_typeddict(value, PullRequest)


def is_pull_requests(value: object) -> TypeGuard[list[PullRequest]]:
    return is_object_list(value) and all(is_pull_request(row) for row in value)


def is_pull_request_pages(value: object) -> TypeGuard[list[list[PullRequest]]]:
    return is_object_list(value) and all(is_pull_requests(page) for page in value)
