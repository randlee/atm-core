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
import sys
from pathlib import Path
from typing import Annotated, Literal

from pydantic import AfterValidator, AliasChoices, BaseModel, ConfigDict, Field, ValidationError, model_validator

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


class SanityMetadata(BaseModel):
    model_config = ConfigDict(extra="allow")
    dev_bead: NonEmpty


class SanityBead(Bead):
    """A sanity bead: it blocks on the bead it checks."""
    metadata: SanityMetadata

    @model_validator(mode="after")
    def blocks_on_dev_bead(self) -> SanityBead:
        if self.metadata.dev_bead not in self.blocks():
            raise ValueError(f"missing blocks edge to its sprint {self.metadata.dev_bead}")
        return self


MODELS: dict[str, type[Bead]] = {"sprint-bead": SprintBead, "sanity-bead": SanityBead}


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


def main(argv: list[str]) -> int:
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
