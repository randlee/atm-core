---
title: AN.9 Template Workflow Contract And Storage Migration
status: planned
branch: feature/an9-template-workflow-contract
target: integrate/phase-an
---

# AN.9 — Template Workflow Contract And Storage Migration

**recommended_agent:** arch-ctm/deep-reasoning (public contract and migration).
**must_follow:** AN.8 merged. This is the public contract consumed by every
later extension sprint; no downstream implementation begins from a draft
shape.
**unblocks:** AN.10.
**parallel_safe:** none. The frontmatter DTO and additive view/migration are
one contract and must land together.

**traceability:** ADR-046; `REQ-P-TEMPLATE-WORKFLOW-001`,
`REQ-P-TEMPLATE-TAGS-001`, `REQ-CORE-TEMPLATE-WORKFLOW-001`, and
`REQ-RUSQLITE-TEMPLATE-WORKFLOW-001`.

## Deliverables

1. Add the validated, backend-neutral `atm-storage` DTOs and the catalog
   frontmatter representation below. Keep workflow strings opaque, bounded
   lower-kebab-case values; do not create `Dev`, `Qa`, `Fix`, or `Sprint`
   enums. `scope.variable` and optional `iteration_variable` are variable
   *names*, not values. A declaration is either complete or absent.

   ```rust
   pub struct TemplateWorkflowDeclaration {
       pub scope_kind: WorkflowScopeKind,
       pub scope_variable: TemplateVariableName,
       pub state: WorkflowState,
       pub stage: WorkflowStage,
       pub transition: WorkflowTransition,
       pub iteration_variable: Option<TemplateVariableName>,
   }

   pub struct TemplateTagDeclaration {
       pub tags: Vec<TemplateTag>,
       pub workflow: Option<TemplateWorkflowDeclaration>,
   }

   pub struct WorkflowSnapshot {
       pub scope_kind: WorkflowScopeKind,
       pub scope_id: WorkflowScopeId,
       pub state: WorkflowState,
       pub stage: WorkflowStage,
       pub transition: WorkflowTransition,
       pub iteration: Option<WorkflowIteration>,
   }
   ```

   The public types validate syntax/length, reject duplicate literal template
   tags and reserved generated prefixes, and expose no SQLite/JSON1 types.
   Existing templates without `metadata.tags` or `metadata.workflow` remain
   valid.
2. Extend template catalog parsing so `metadata.tags` and the complete
   `metadata.workflow` declaration are captured in immutable canonical schema
   data. A partial object, a template expression inside a tag, a duplicate
   tag, or a caller/template tag beginning with a reserved generated prefix is
   a typed validation error before catalog or mail mutation.
3. Add one idempotent `atm-storage-rusqlite` migration and versioned additive
   `decomposed_messages` view projection for:
   `workflow_scope_kind`, `workflow_scope_id`, `workflow_state`,
   `workflow_stage`, `workflow_transition`, `workflow_iteration`,
   `applied_template_tags_json`, and `effective_tags_json`. Existing
   `tags_json` is unchanged and remains caller/instance tags.
4. Add the transport-neutral, pure `atm-core` resolver/validator signature
   consumed by AN.10: it accepts a complete `TemplateWorkflowDeclaration` and
   the already-merged variables, returning `WorkflowSnapshot` or the typed
   value-validation error. It performs no storage I/O. The concrete store only
   receives that validated snapshot to persist atomically.
5. Extend the existing sealed `TemplateCatalogStore` admission contract with
   the AN.10 shape, without adding another optional storage capability trait
   under ADR-036. It must not expose SQL, FTS syntax, or an unsealed extension
   point. The concrete adapter remains the sole owner of migrations,
   transactions, and indexes.
6. Update the author guide and crate architecture/requirement references only
   where the landed public contract changes them. The guide is the one source
   of truth for workflow authoring conventions. Register ADR-046's template
   validation error codes in `docs/atm-error-codes.md` with the specified
   cause/recovery guidance.

## Acceptance criteria

- A template with no workflow metadata behaves byte-for-byte compatibly with
  the AN.8 catalog/admission contract.
- Complete declarations round-trip through catalog storage canonically; every
  partial/invalid declaration fails before a template or message row changes.
- The migration is idempotent, preserves an AN.8 fixture database, and the
  view returns `NULL` snapshot fields for pre-extension rows.
- The new DTOs are semantic newtypes/validated values at the public boundary;
  no raw unbounded `String` or SQLite type leaks into the capability trait.
- The pure core resolver accepts valid scalar merged values and rejects a
  missing, null, non-scalar, empty, or out-of-bounds declared value before the
  storage capability is called.
- The reserved prefixes are exactly those in ADR-046 and cannot be spoofed by
  either template metadata or caller-provided instance tags.

## Required validation

- unit tests for valid opaque non-dev vocabulary plus every invalid/partial
  declaration class
- catalog round-trip and migration/reopen tests using an AN.8 database fixture
- view compatibility test for legacy/decomposed/plain rows
- boundary lint and `cargo test -p atm-storage -p atm-storage-rusqlite`
- `just test`

## Paths to delete

None. This is an additive contract/migration sprint; compatibility columns and
the prior decomposed view remain supported.

## Non-closure

AN.9 defines the pure resolver and persists the shape only. It does **not**
wire resolver output into admission, populate snapshots for new messages, add
query filters, pair lifecycles, or export telemetry. Those are AN.10 and AN.11
work.
