# Generated from `bd schema` (bd 1.3.1), types.issue; do not edit by hand.
# Regenerate: bd schema | jq .types.issue > issue.json && uvx --from datamodel-code-generator==0.34.0 datamodel-codegen \
#   --input issue.json --input-file-type jsonschema --output-model-type pydantic_v2.BaseModel --class-name Issue \
#   --target-python-version 3.9 --use-standard-collections --disable-timestamp
# then update BD_VERSION and BD_SCHEMA_SHA256 (sha256 of the full `bd schema` output).
from __future__ import annotations

from enum import Enum
from typing import Any, Optional

from pydantic import AwareDatetime, BaseModel, ConfigDict


class Status(Enum):
    open = 'open'
    in_progress = 'in_progress'
    blocked = 'blocked'
    deferred = 'deferred'
    closed = 'closed'
    pinned = 'pinned'
    hooked = 'hooked'


class IssueType(Enum):
    bug = 'bug'
    feature = 'feature'
    task = 'task'
    epic = 'epic'
    chore = 'chore'
    decision = 'decision'
    message = 'message'
    molecule = 'molecule'
    gate = 'gate'
    spike = 'spike'
    story = 'story'
    milestone = 'milestone'


class Type(Enum):
    blocks = 'blocks'
    parent_child = 'parent-child'
    conditional_blocks = 'conditional-blocks'
    waits_for = 'waits-for'
    related = 'related'
    discovered_from = 'discovered-from'
    replies_to = 'replies-to'
    relates_to = 'relates-to'
    duplicates = 'duplicates'
    supersedes = 'supersedes'
    authored_by = 'authored-by'
    assigned_to = 'assigned-to'
    approved_by = 'approved-by'
    attests = 'attests'
    tracks = 'tracks'
    until = 'until'
    caused_by = 'caused-by'
    validates = 'validates'
    delegated_from = 'delegated-from'


class Dependency(BaseModel):
    model_config = ConfigDict(
        extra='forbid',
    )
    id: Optional[str] = None
    issue_id: str
    depends_on_id: str
    type: Type
    created_at: AwareDatetime
    created_by: Optional[str] = None
    metadata: Optional[str] = None
    thread_id: Optional[str] = None


class Comment(BaseModel):
    model_config = ConfigDict(
        extra='forbid',
    )
    id: str
    issue_id: str
    author: str
    text: str
    created_at: AwareDatetime


class BondedFromItem(BaseModel):
    model_config = ConfigDict(
        extra='forbid',
    )
    source_id: str
    bond_type: str
    bond_point: Optional[str] = None


class Issue(BaseModel):
    model_config = ConfigDict(
        extra='forbid',
    )
    id: str
    title: str
    description: Optional[str] = None
    design: Optional[str] = None
    acceptance_criteria: Optional[str] = None
    notes: Optional[str] = None
    spec_id: Optional[str] = None
    status: Optional[Status] = None
    priority: int
    issue_type: Optional[IssueType] = None
    is_blocked: Optional[bool] = None
    assignee: Optional[str] = None
    owner: Optional[str] = None
    estimated_minutes: Optional[int] = None
    created_at: AwareDatetime
    created_by: Optional[str] = None
    updated_at: AwareDatetime
    started_at: Optional[AwareDatetime] = None
    closed_at: Optional[AwareDatetime] = None
    close_reason: Optional[str] = None
    closed_by_session: Optional[str] = None
    lease_expires_at: Optional[AwareDatetime] = None
    heartbeat_at: Optional[AwareDatetime] = None
    lease_granted_node: Optional[str] = None
    due_at: Optional[AwareDatetime] = None
    defer_until: Optional[AwareDatetime] = None
    external_ref: Optional[str] = None
    source_system: Optional[str] = None
    metadata: Optional[Any] = None
    compaction_level: Optional[int] = None
    compacted_at: Optional[AwareDatetime] = None
    compacted_at_commit: Optional[str] = None
    original_size: Optional[int] = None
    labels: Optional[list[str]] = None
    dependencies: Optional[list[Dependency]] = None
    comments: Optional[list[Comment]] = None
    sender: Optional[str] = None
    ephemeral: Optional[bool] = None
    no_history: Optional[bool] = None
    wisp_type: Optional[str] = None
    storage_class: Optional[str] = None
    pinned: Optional[bool] = None
    is_template: Optional[bool] = None
    bonded_from: Optional[list[BondedFromItem]] = None
    await_type: Optional[str] = None
    await_id: Optional[str] = None
    timeout: Optional[int] = None
    waiters: Optional[list[str]] = None
    source_formula: Optional[str] = None
    source_location: Optional[str] = None
    mol_type: Optional[str] = None
    work_type: Optional[str] = None
    event_kind: Optional[str] = None
    actor: Optional[str] = None
    target: Optional[str] = None
    payload: Optional[str] = None


BD_VERSION = "1.3.1"
BD_SCHEMA_VERSION = 1
BD_SCHEMA_SHA256 = "caf8fe18e1117c067ff7bbd824484b24849677e2cbf86d793ddf6b64069a31a2"
