# atm-bd-types 0.4.0 framework surface: the same for every profile. Policy loading, decision record, view,
# executor outcomes, ports. The framework executes the Actions a plug-in decides; plug-ins never import this.
import builtins
import enum
import typing

from atm_bd_types import (Action, Bead, BeadStore, CheckState, CollectError, Layer, Merged, Params, PhaseFacts, QaFail, QaPass, Refusal, Stack, Stage, Status,
                          TaskState)

_T = typing.TypeVar("_T")

@typing.dataclass_transform(frozen_default=True)
def _record(cls: type[_T]) -> type[_T]:
    r"""Stub-only: a frozen Rust record whose constructor takes every field in order."""


class ActionKind(enum.Enum):
    ASSIGN = ...
    REPORT = ...
    NOTIFY = ...

class OutcomeKind(enum.Enum):
    SENT = ...
    FAILED = ...
    DRY_RUN = ...

@_record
class Policy:
    r"""policy.toml, loaded before any read: POLICY_BAD_PARAMS (a param neither int nor text), PROFILE_MISMATCH."""
    profile: str
    params: Params
    notify_template: str

@_record
class ViewRow:
    queue: str
    key: str
    bead_id: str | None
    text: str
    since: int | None

@_record
class Decision:
    r"""One tick's decide result after dedupe: actions to execute; Reports already sent stay rows only."""
    actions: list[Action]
    rows: list[ViewRow]

@_record
class ViewStep:
    bead_id: str
    stage: Stage
    status: Status
    assignee: str | None
    ready: bool

@_record
class ViewSprint:
    id: str
    title: str
    status: Status
    depends_on: list[str]
    steps: list[ViewStep]

@_record
class ViewTask:
    task_id: str
    assignee: str
    state: TaskState
    reminders: int
    since: int | None

@_record
class ViewAction:
    r"""outcome None: planned, not executed."""
    kind: ActionKind
    to: str
    bead_id: str | None
    key: str
    outcome: OutcomeKind | None

@_record
class PhaseView:
    phase: str
    root_id: str
    root_closed: bool
    lead: str
    generated_at: int
    sprints: list[ViewSprint]
    tasks: list[ViewTask]
    rows: list[ViewRow]
    actions: list[ViewAction]
    top_ci: CheckState | None
    errors: list[CollectError]

@_record
class ActionOutcome:
    action: Action
    kind: OutcomeKind
    failure: Refusal | None

@_record
class TickResult:
    facts: PhaseFacts
    decision: Decision
    view: PhaseView
    outcomes: list[ActionOutcome]

@_record
class CommandResult:
    code: int
    stdout: str
    stderr: str

class CommandRunner(typing.Protocol):
    r"""The only process port: collectors read through it, the executor writes through it."""
    def run(self, argv: list[str], stdin: str | None) -> CommandResult: ...

class BeadsCache(BeadStore, typing.Protocol):
    r"""show() from BeadStore plus the phase's listed and ready beads."""
    def list(self, phase: str) -> builtins.list[Bead] | Refusal: ...
    def ready(self, phase: str) -> builtins.list[Bead] | Refusal: ...

@_record
class Ports:
    runner: CommandRunner
    cache: BeadsCache

def stack_of(bead: Bead) -> Stack | None: ...

def encode_evidence(evidence: Layer | QaPass | QaFail | Merged) -> str: ...
