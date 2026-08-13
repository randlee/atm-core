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

1. On a same-host, same-team decomposed templated admission, call AN.9's pure
   `atm-core` resolver on declared scope and optional iteration using the
   normal merged variables. The resolver's non-empty bounded scalar output is
   passed to storage; missing, null, object, array, or invalid values fail with
   a typed template-workflow validation error before the storage transaction
   begins. No rendered prose is parsed and `atm-storage-rusqlite` does not own
   variable interpretation.
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
   only. Instance tags and applied template tags are durable source sets; the
   derived set is reproduced exactly from the immutable snapshot and admitted
   template type/content format, so no redundant `derived_tags_json` is
   stored. Derive exactly ADR-046's `template-type:`, `content-format:`, and
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

- A successful eligible admission atomically produces every snapshot field,
  the two immutable input tag sets, and their effective projection; an induced
  error leaves neither a partial mail row nor an orphan catalog mutation.
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
- admission-construction mismatch-rejection test: recomputing the canonical
  effective projection from instance tags, applied-template tags, and derived
  snapshot tags must reject any supplied/stored mismatch before commit
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
