#!/usr/bin/env python3
# DO NOT EDIT WITHOUT EXPLICIT USER APPROVAL.
# A bead that fails these models is fixed in the bead, never by changing the models. Report the problem instead.
"""Pydantic models of the schema-type beads. The published JSON Schemas (atm-beads/schemas/) are exported from them.

  bead_schema.py validate <beads.json> <plan.jsonl> <id prefix>      one "<bead>: <field>: <problem>" line per problem
  bead_schema.py export <dir>                                          write <dir>/<name>.schema.json for every model

Exit 0 valid, 5 problems, 2 could not run.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Annotated, ClassVar, Literal

from pydantic import AfterValidator, AliasChoices, BaseModel, ConfigDict, Field, ValidationError, field_validator, model_validator

# the description holds a numbered '## Deliverables' list starting at 1
DELIVERABLES = r"(?m)^## Deliverables\s*\n(?:\s*\n)*\s*1[.)]\s+\S"
NonEmpty = Annotated[str, Field(min_length=1)]


def ids_or_none(v: list[str]) -> list[str]:
    if not v:
        raise ValueError('list the ids, or exactly ["NONE"]')
    if "NONE" in v and len(v) > 1:
        raise ValueError("mixes NONE with ids")
    return v


IdList = Annotated[list[NonEmpty], AfterValidator(ids_or_none)]


class Dependency(BaseModel):
    model_config = ConfigDict(extra="allow")
    type: str = Field(validation_alias=AliasChoices("type", "dependency_type"))
    depends_on_id: str = Field(validation_alias=AliasChoices("depends_on_id", "id"))


class Bead(BaseModel):
    model_config = ConfigDict(extra="allow")
    id: NonEmpty
    dependencies: list[Dependency] = []

    def blocks(self) -> list[str]:
        return [d.depends_on_id for d in self.dependencies if d.type == "blocks"]


class SprintMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    requirements: IdList
    adrs: IdList
    worktree: NonEmpty
    branch: NonEmpty
    pr_target: NonEmpty
    difficulty: Literal["hard", "normal", "fast"]


class SprintBead(Bead):
    """A stage:sprint container: the plan fields; its dev, sanity and qa beads are poured under it."""
    description: Annotated[str, Field(pattern=DELIVERABLES)]
    acceptance_criteria: NonEmpty
    metadata: SprintMetadata


class FamilyMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    branch: NonEmpty
    worktree: NonEmpty
    pr_target: NonEmpty
    difficulty: Literal["hard", "normal", "fast"]


class CloseFields(BaseModel):
    """Stamped at pour on every open dev, fix, sanity and QA bead (close_fields.POURED): its close reads no other bead."""
    model_config = ConfigDict(extra="allow")
    role: Literal["dev", "fix", "sanity", "qa"]
    phase: NonEmpty
    sprint: NonEmpty
    sprint_bead: NonEmpty
    stack: NonEmpty
    stack_bead: NonEmpty
    branch: NonEmpty
    worktree: NonEmpty
    pr_target: NonEmpty
    base: NonEmpty
    checked_bead: NonEmpty
    checked_title: NonEmpty
    sanity_bead: str = ""
    qa_bead: NonEmpty
    phase_feature: NonEmpty
    lint_command: NonEmpty
    policy_path: NonEmpty
    max_sanity_rounds: int = Field(ge=1)

    @model_validator(mode="after")
    def names_its_sanity_bead(self) -> CloseFields:
        if not self.sanity_bead and not (self.model_extra or {}).get("quick_fix"):
            raise ValueError("sanity_bead is empty; only a quick-fix QA bead has no sanity bead")
        return self


class TaskBead(Bead):
    """A dev, fix, sanity or QA bead; while it is not closed its metadata carries CloseFields for `close_role`.
    A finding bead (`stamped_at_dispatch`) gets them when the lead dispatches it, so only an in-progress one is checked."""
    close_role: ClassVar[str] = ""
    stamped_at_dispatch: ClassVar[bool] = False

    @model_validator(mode="after")
    def carries_close_fields(self) -> TaskBead:
        status = getattr(self, "status", None)
        if status == "closed" or (self.stamped_at_dispatch and status != "in_progress"):
            return self
        fields = CloseFields.model_validate(getattr(self, "metadata").model_dump())
        if fields.role != self.close_role:
            raise ValueError(f"metadata.role is {fields.role!r}; a {self.close_role} bead needs {self.close_role!r}")
        return self


class DevBead(TaskBead):
    close_role: ClassVar[str] = "dev"
    metadata: FamilyMetadata


class FixMetadata(FamilyMetadata):
    finding_ref: NonEmpty
    severity: Literal["blocking"]
    reviewer: NonEmpty
    found_at_commit: NonEmpty


class FixBead(TaskBead):
    close_role: ClassVar[str] = "fix"
    metadata: FixMetadata


class SanityMetadata(FamilyMetadata):
    model_config = ConfigDict(extra="allow")
    dev_bead: NonEmpty


class SanityBead(TaskBead):
    """A sanity bead links once to the bead it checks."""
    close_role: ClassVar[str] = "sanity"
    metadata: SanityMetadata

    @model_validator(mode="after")
    def blocks_on_dev_bead(self) -> SanityBead:
        related = self.blocks()
        count = related.count(self.metadata.dev_bead)
        if count == 0:
            raise ValueError(f"missing blocks edge to its sprint {self.metadata.dev_bead}")
        if count > 1:
            raise ValueError(f"multiple blocks edges to {self.metadata.dev_bead}")
        return self


class QaMetadata(FamilyMetadata):
    checked_bead: NonEmpty
    round: int = Field(ge=1)
    reviewers_round1: Annotated[list[Annotated[str, Field(pattern=r"^[a-z][a-z0-9_-]*$")]],
                               Field(min_length=1, json_schema_extra={"uniqueItems": True})]

    @field_validator("reviewers_round1")
    @classmethod
    def unique_reviewers(cls, reviewers: list[str]) -> list[str]:
        if len(set(reviewers)) != len(reviewers):
            raise ValueError("duplicate reviewer identities")
        return reviewers


class QaBead(TaskBead):
    close_role: ClassVar[str] = "qa"
    metadata: QaMetadata


class RootMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    integration_branch: NonEmpty


class RootBead(Bead):
    metadata: RootMetadata


class GateBead(Bead):
    pass


class FindingMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    severity: Literal["blocking", "important", "minor"]
    reviewer: NonEmpty
    found_at_commit: NonEmpty


class FindingBead(TaskBead):
    close_role: ClassVar[str] = "fix"
    stamped_at_dispatch: ClassVar[bool] = True
    metadata: FindingMetadata


class LayerRecord(BaseModel):
    model_config = ConfigDict(extra="allow")
    branch: NonEmpty
    pr: int
    head: NonEmpty
    base: NonEmpty
    linked: bool


class StackMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    trunk: NonEmpty
    stack_number: int | None
    layers: list[LayerRecord]


class StackBead(Bead):
    metadata: StackMetadata


MODELS_BY_STAGE: dict[str, type[Bead]] = {
    "stage:sprint": SprintBead, "stage:dev": DevBead, "stage:dev-sanity": SanityBead,
    "stage:qa": QaBead, "stage:fix": FixBead, "stage:finding": FindingBead,
    "stage:stack": StackBead, "stage:plan": Bead, "stage:plan-review": Bead, "stage:review": Bead,
}
MODELS: dict[str, type[Bead]] = {
    "sprint-bead": SprintBead, "dev-bead": DevBead, "sanity-bead": SanityBead,
    "qa-bead": QaBead, "fix-bead": FixBead, "finding-bead": FindingBead,
    "stack-bead": StackBead, "root-bead": RootBead, "gate-bead": GateBead,
}


def problems(bead: dict, model: type[Bead]) -> list[str]:
    try:
        model.model_validate(bead)
    except ValidationError as exc:
        out = []
        for e in exc.errors():
            where = ".".join(str(p) for p in e["loc"])
            msg = e["msg"].removeprefix("Value error, ")
            out.append(f"{bead.get('id', '?')}: {where + ': ' if where else ''}{msg}")
        return out
    return []


def validate_phase(beads: list[dict]) -> list[str]:
    found = []
    for bead in beads:
        if "anchor" in (bead.get("labels") or []):
            continue
        bead_id = str(bead.get("id", "?"))
        if re.fullmatch(r".+-phase-[a-z0-9]+", bead_id):
            model = RootBead
        elif bead.get("issue_type") == "gate":
            model = GateBead
        else:
            stages = [label for label in bead.get("labels") or [] if label.startswith("stage:")]
            model = MODELS_BY_STAGE.get(stages[0]) if len(stages) == 1 else None
        if model is None:
            found.append(f"{bead_id}: no model")
        else:
            found.extend(problem.replace(f"{bead_id}: metadata.", f"{bead_id}: ", 1)
                         for problem in problems(bead, model))
    return found


def load_phase(phase: str) -> list[dict]:
    if not re.fullmatch(r"[a-z0-9]+", phase):
        raise ValueError(f"invalid phase {phase!r}")
    proc = subprocess.run(["bd", "list", "-l", f"phase-{phase}", "--status",
                           "open,in_progress,blocked,deferred,closed", "--include-gates", "-n", "0", "--json"],
                          capture_output=True, text=True)
    if proc.returncode:
        raise ValueError(f"bd list failed: {proc.stderr.strip()}")
    rows = json.loads(proc.stdout)
    if not isinstance(rows, list):
        raise ValueError("bd list did not return a list")
    return rows


def main(argv: list[str]) -> int:
    if len(argv) == 3 and argv[:2] == ["validate", "--phase"]:
        found = validate_phase(load_phase(argv[2]))
        if found:
            print("\n".join(found))
        return 5 if found else 0
    if len(argv) == 4 and argv[0] == "validate":
        beads = {b.get("id"): b for b in json.loads(Path(argv[1]).read_text())}
        plan = [json.loads(line) for line in Path(argv[2]).read_text().splitlines() if line.strip()]
        found = []
        for row in plan:
            container = f"{argv[3]}{row['sprint']}"
            for bid, model in ((container, SprintBead), (f"{container}.group-sanity", SanityBead)):
                if bid in beads:
                    found += problems(beads[bid], model)
        print("\n".join(found)) if found else None
        return 5 if found else 0
    if len(argv) == 2 and argv[0] == "export":
        for name, model in MODELS.items():
            Path(argv[1], f"{name}.schema.json").write_text(json.dumps(model.model_json_schema(), indent=2) + "\n")
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except (OSError, ValueError) as exc:
        print(f"bead_schema: {exc}", file=sys.stderr)
        raise SystemExit(2)
