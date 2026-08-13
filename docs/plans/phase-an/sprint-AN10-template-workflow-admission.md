---
title: AN.10 Atomic Workflow Admission Snapshots And Tag Provenance
status: planned
branch: feature/an10-template-workflow-admission
target: integrate/phase-an
---

# AN.10 — Atomic Workflow Admission Snapshots And Tag Provenance

**recommended_agent:** arch-ctm/deep-reasoning (admission transaction and
provenance).
**must_follow:** AN.9 merged; merge its integration tip before every dev/fix
round because this sprint implements AN.9's frozen DTO/migration contract.
**unblocks:** AN.11.
**parallel_safe:** none. Admission and storage transaction semantics share the
same rows and cannot be independently closed.

**traceability:** ADR-046; `REQ-P-TEMPLATE-WORKFLOW-001`,
`REQ-P-TEMPLATE-TAGS-001`, `REQ-CORE-TEMPLATE-WORKFLOW-001`, and
`REQ-RUSQLITE-TEMPLATE-WORKFLOW-001`.

## Deliverables

1. On a same-host, same-team decomposed templated admission, resolve declared
   scope and optional iteration from the already-persisted merged variables.
   Values must be non-empty bounded strings; missing, null, object, array, or
   invalid values fail with a typed template-workflow validation error before
   the mail row is visible. No rendered prose is parsed.
2. Persist the `WorkflowSnapshot`, canonical applied-template tags, and
   deterministic effective tags in the *same* concrete-store transaction that
   registers/reuses the template and inserts the decomposed message.

   ```rust
   pub struct MessageTagProvenance {
       pub instance_tags: Vec<InstanceTag>,
       pub applied_template_tags: Vec<TemplateTag>,
       pub derived_tags: Vec<DerivedTag>,
       pub effective_tags: Vec<EffectiveTag>,
   }
   ```

   `effective_tags` is a canonical duplicate-free lexical union for search
   only. The three source sets and workflow snapshot remain durable source
   facts. Derive exactly ADR-046's `template-type:`, `content-format:`, and
   `workflow-*:` tags; values absent from an existing template simply omit the
   corresponding derived tag.
3. Preserve routing behavior. Only the same-host/same-team decomposed cell
   has a snapshot. Same-team/cross-host, foreign-team/same-host,
   foreign-team/cross-host, plain, legacy, and include-fallback sends retain
   current rendered/plain behavior and never invent a workflow snapshot.
4. Preserve immutable history. A new template SHA with different tags or
   workflow metadata affects only later admissions; it never changes stored
   rows made with the earlier SHA. Caller instance tags likewise remain
   distinguishable from template classification.

## Acceptance criteria

- A successful eligible admission produces all snapshot fields and all three
  tag provenances atomically; an induced error leaves neither a partial mail
  row nor an orphan catalog mutation.
- Same message body/variables with two template revisions yields distinct,
  historically stable applied/effective tags and snapshots.
- The four routing cells retain AN.8 behavior, with storage assertions rather
  than output-only assertions; only the eligible cell has snapshot fields.
- Existing `tags_json` callers retain behavior; no API treats effective tags
  as caller input or permits a reserved derived prefix through either source.
- No legacy daemon file, cross-host template-sync mechanism, or telemetry
  dependency is changed.

## Required validation

- transaction rollback/property tests and deterministic tag-union tests
- revision-history and instance-vs-template provenance regression tests
- same-host/cross-host and team-boundary routing matrix on Tokio/Axum runtime
- direct storage tests proving no snapshot for plain/legacy/include fallback
- `cargo test` for affected crates, boundary lint, and `just test`

## Paths to delete

None. This sprint replaces no supported admission path and must not remove the
plain/legacy fallback behavior.

## Non-closure

AN.10 materializes facts but exposes no new local query grammar, lifecycle
pairing, or telemetry exporter. Those are AN.11 work.
