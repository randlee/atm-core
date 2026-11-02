"""Test-only, in-memory implementation of the atm-bd-types 0.2.0 stub (typings/atm_bd_types/__init__.pyi).

It exists so the three lite hooks execute under pytest before the Rust crate is published. It implements
exactly the stub's surface, nothing more; behaviour follows the docstrings in the stub and the Transitions
tables in docs/plans/phase-bo/hooks/*.md. It is installed into sys.modules as `atm_bd_types` by conftest.
"""
from __future__ import annotations

import enum
import json
from dataclasses import dataclass, field
from typing import Protocol


class Status(enum.Enum):
    OPEN = "open"
    IN_PROGRESS = "in_progress"
    BLOCKED = "blocked"
    DEFERRED = "deferred"
    CLOSED = "closed"


class Template(enum.Enum):
    REFUSE = "refuse"
    DEV_CLOSE = "dev-close"
    QA_PASS = "qa-pass"
    QA_FINDINGS = "qa-findings"


class ErrorId(enum.Enum):
    REQUEST_INVALID = "REQUEST_INVALID"
    PHASE_UNREADABLE = "PHASE_UNREADABLE"
    PROFILE_MISMATCH = "PROFILE_MISMATCH"
    CACHE_UNAVAILABLE = "CACHE_UNAVAILABLE"
    BEAD_NOT_FOUND = "BEAD_NOT_FOUND"
    BEAD_NOT_READY = "BEAD_NOT_READY"
    BEAD_CLOSED = "BEAD_CLOSED"
    BEAD_RACED = "BEAD_RACED"
    ASSIGN_NOT_LEAD = "ASSIGN_NOT_LEAD"
    TASK_NOT_ASSIGNEE = "TASK_NOT_ASSIGNEE"
    CLOSE_REASON_EMPTY = "CLOSE_REASON_EMPTY"
    CLOSE_REPORT_NOT_INLINE = "CLOSE_REPORT_NOT_INLINE"
    STACK_NOT_RECORDED = "STACK_NOT_RECORDED"
    QA_VERDICT = "QA_VERDICT"
    QA_NOT_ON_STACK = "QA_NOT_ON_STACK"
    QA_NOTHING_TO_REVIEW = "QA_NOTHING_TO_REVIEW"
    FINDINGS_NOT_POURED = "FINDINGS_NOT_POURED"


class Refusal:
    def __init__(self, id: ErrorId, **context: str | list[str]) -> None:
        self._id, self.context = id, context

    @property
    def id(self) -> ErrorId:
        return self._id

    def __repr__(self) -> str:
        return f"Refusal({self._id.name}, {self.context})"


@dataclass(frozen=True)
class Layer:
    branch: str
    head: str


@dataclass(frozen=True)
class Finding:
    id: str
    blocking: bool


@dataclass(frozen=True)
class QaPass:
    sha: str


@dataclass(frozen=True)
class QaFail:
    sha: str
    findings: list[Finding]


@dataclass
class Bead:
    id: str
    status: Status = Status.OPEN
    assignee: str | None = None
    open_blockers: list[str] = field(default_factory=list)
    branch: str = ""
    stack_bead: str = ""
    sprint: str = ""
    phase: str = ""
    # not in the stub's Python surface; the Rust crate reads these from the bead's labels/metadata
    stage: str = "dev"
    blocks_ids: list[str] = field(default_factory=list)
    layers: dict[str, Layer] = field(default_factory=dict)

    def blocks(self, bead_id: str) -> bool:
        return bead_id in self.blocks_ids

    def layer_for(self, branch: str) -> Layer | None:
        return self.layers.get(branch)


@dataclass(frozen=True)
class PhaseFile:
    lead: str

    def template(self, which: Template) -> str:
        return f"templates/{which.value}.md"


class AssignTransition(enum.Enum):
    ASSIGN_SAME = 1
    ASSIGN = 2
    ASSIGN_CLOSED = 3
    ASSIGN_NOT_READY = 4

    @staticmethod
    def select(status: Status, assignee: str | None, wanted: str) -> AssignTransition:
        if status is Status.CLOSED:
            return AssignTransition.ASSIGN_CLOSED
        if status in (Status.BLOCKED, Status.DEFERRED):
            return AssignTransition.ASSIGN_NOT_READY
        return AssignTransition.ASSIGN_SAME if assignee == wanted else AssignTransition.ASSIGN


class StartTransition(enum.Enum):
    START = 1
    START_AGAIN = 2
    START_CLOSED = 3
    START_NOT_READY = 4

    @staticmethod
    def select(status: Status) -> StartTransition:
        return {Status.OPEN: StartTransition.START, Status.IN_PROGRESS: StartTransition.START_AGAIN,
                Status.CLOSED: StartTransition.START_CLOSED}.get(status, StartTransition.START_NOT_READY)


class CloseTransition(enum.Enum):
    GIVE_BACK = 1
    WORK_CLOSE = 2
    QA_PASS = 3
    QA_FINDINGS = 4

    @staticmethod
    def select(req: HookRequest, bead: Bead) -> CloseTransition | Refusal:
        if req.outcome in ("refused", "cancelled"):
            return CloseTransition.GIVE_BACK
        if req.outcome != "completed":
            return Refusal(ErrorId.REQUEST_INVALID, field="outcome")
        if bead.stage in ("dev", "fix"):
            return CloseTransition.WORK_CLOSE
        if bead.stage == "qa":
            if req.verdict is None:
                return Refusal(ErrorId.QA_VERDICT)
            return CloseTransition.QA_PASS if req.verdict == "PASS" else CloseTransition.QA_FINDINGS
        return Refusal(ErrorId.REQUEST_INVALID, field="stage")


@dataclass(frozen=True)
class BeadPatch:
    kind: str
    snapshot: Bead
    arg: object = None

    @staticmethod
    def assign(snapshot: Bead, agent: str) -> BeadPatch:
        return BeadPatch("assign", snapshot, agent)

    @staticmethod
    def start(snapshot: Bead) -> BeadPatch:
        return BeadPatch("start", snapshot)

    @staticmethod
    def reopen(snapshot: Bead) -> BeadPatch:
        return BeadPatch("reopen", snapshot)

    @staticmethod
    def close(snapshot: Bead, evidence: Layer | QaPass | QaFail) -> BeadPatch:
        return BeadPatch("close", snapshot, evidence)


@dataclass
class HookRequest:
    phase: str = "bo"
    phase_config_path: str = ".atm-bd/phase-bo.toml"
    caller: str = "dev"
    task_id: str = "bo-10"
    assignee: str | None = None
    outcome: str = "completed"
    close_report: str | None = None
    close_reason: str | None = None
    phase_task: bool = True
    verdict: str | None = None          # "PASS" | "FINDINGS" | None
    verdict_sha: str = ""
    findings: list[Finding] = field(default_factory=list)

    @staticmethod
    def parse(stdin: str) -> HookRequest | Refusal:
        try:
            return HookRequest(**json.loads(stdin))
        except (ValueError, TypeError):
            return Refusal(ErrorId.REQUEST_INVALID, field="stdin")

    def is_phase_task(self) -> bool:
        return self.phase_task

    def qa_pass(self) -> QaPass | Refusal:
        return QaPass(self.verdict_sha) if self.verdict == "PASS" and self.verdict_sha else Refusal(ErrorId.QA_VERDICT)

    def qa_findings(self) -> QaFail | Refusal:
        if self.verdict != "FINDINGS" or not self.verdict_sha:
            return Refusal(ErrorId.QA_VERDICT)
        return QaFail(self.verdict_sha, list(self.findings))


@dataclass(frozen=True)
class HookAction:
    template: str
    vars: object

    @staticmethod
    def close(template: str, vars: Layer | QaPass | QaFail | str) -> HookAction:
        return HookAction(template, vars)


@dataclass(frozen=True)
class HookReply:
    kind: str
    refusal: Refusal | None = None
    actions: tuple[HookAction, ...] = ()

    @staticmethod
    def not_applicable() -> HookReply:
        return HookReply("not-applicable")

    @staticmethod
    def pass_through() -> HookReply:
        return HookReply("pass-through")

    @staticmethod
    def refused(refusal: Refusal) -> HookReply:
        return HookReply("refused", refusal)

    @staticmethod
    def handled(actions: list[HookAction]) -> HookReply:
        return HookReply("handled", None, tuple(actions))

    def to_json(self) -> str:
        body: dict[str, object] = {"outcome": self.kind}
        if self.refusal is not None:
            body["code"] = self.refusal.id.name
            body["context"] = self.refusal.context
        if self.actions:
            body["actions"] = [{"template": a.template, "vars": str(a.vars)} for a in self.actions]
        return json.dumps(body, sort_keys=True)


class BeadStore(Protocol):
    def show(self, bead_id: str) -> Bead | Refusal: ...
    def target_layer(self, qa_bead: Bead) -> Layer | Refusal: ...
    def update(self, patch: BeadPatch, actor: str) -> Refusal | None: ...


class PhaseFiles(Protocol):
    def load(self, path: str, profile: str) -> PhaseFile | Refusal: ...


class CacheStore:
    """Never used by tests; present so the hooks' __main__ imports resolve."""

    @staticmethod
    def for_phase(phase: str) -> CacheStore | Refusal:
        return Refusal(ErrorId.CACHE_UNAVAILABLE, phase=phase)

    def show(self, bead_id: str) -> Bead | Refusal:
        return Refusal(ErrorId.CACHE_UNAVAILABLE)

    def target_layer(self, qa_bead: Bead) -> Layer | Refusal:
        return Refusal(ErrorId.CACHE_UNAVAILABLE)

    def update(self, patch: BeadPatch, actor: str) -> Refusal | None:
        return Refusal(ErrorId.CACHE_UNAVAILABLE)


class PhaseFilesCli:
    def load(self, path: str, profile: str) -> PhaseFile | Refusal:
        return Refusal(ErrorId.PHASE_UNREADABLE, path=path)
