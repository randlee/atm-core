---
title: AN.5 Search Infrastructure — FTS Indexes and Sync
status: draft
branch: feature/pan-s5-search-infrastructure
worktree: ../atm-core-worktrees/feature/pan-s5-search-infrastructure
target: integrate/phase-an
---

# AN.5 — Search Infrastructure: FTS Indexes and Sync

**recommended_agent:** arch-ctm/deep-reasoning (index-consistency contract
under all write paths).
**must_follow:** AN.2; merge AN.2's pushed integration line before each dev or
fix round.
**unblocks:** AN.6.
**parallel_safe:** AN.3 and AN.4 (this sprint owns index DDL and
storage-layer index sync in `atm-storage-rusqlite`; it does not touch send or
read surfaces).

**traceability:** plan-phase-an.md Decision 10 (FTS scope), Query surface
section; FTS external-content drift risk entry. Requirement IDs assigned
during plan hardening.

## Deliverables

1. Message FTS index (FTS5) covering, per row: `message_text` (plain and
   file-ref rows only), `summary`, flattened tags, and — for decomposed rows
   — flattened **var values** from `vars_json`:

```sql
CREATE VIRTUAL TABLE IF NOT EXISTS mail_messages_fts USING fts5(
    body_text,        -- message_text for plain/file-ref rows, '' for decomposed
    summary,
    tags,             -- space-joined tags_json
    var_values,       -- deterministic flattening of vars_json values ('' for plain)
    from_agent,
    content='',       -- contentless-delete style; storage layer owns sync
    tokenize='unicode61 remove_diacritics 2'
);
```

   Var-value flattening is deterministic: values in key-sorted order, scalar
   values verbatim, arrays flattened in order, objects recursed in key order.
2. Template-content FTS index over `message_templates.content`, enabling
   boilerplate search at the template layer with SHA-join expansion to
   messages (consumed by AN.6).
3. Index synchronization owned by the storage-layer write paths introduced in
   AN.2 — the same code path that writes `mail_messages`/`message_templates`
   maintains the indexes in the same transaction. No write path may bypass
   sync (property-tested).
4. Backfill migration populating both indexes from existing rows, plus
   `atm admin reindex-search` rebuilding both from scratch for recovery.
5. Drift property tests: arbitrary interleavings of insert/update/delete
   across all three `MessageBody` variants leave the indexes exactly
   consistent with a from-scratch rebuild.
6. Introduce the separate, sealed `MessageSearchStore` capability in
   `atm-storage`. It owns backend-neutral typed search filters, simple
   aggregates, pages, and results, and is intentionally separate from
   `MessageStore`. The `atm-storage-rusqlite` implementation compiles those
   types to FTS5/JSON1 privately. It exposes no SQL strings, raw FTS syntax,
   `rusqlite` types, HTTP DTOs, or renderer handles. Contract tests run the
   capability against a fake/in-memory implementation; this sprint retains
   SQLite parity and index-consistency tests.

## Acceptance criteria

- After any property-test interleaving, `mail_messages_fts` and the template
  index match a fresh `reindex-search` rebuild row-for-row.
- Backfill on a migrated historical fixture DB equals a post-backfill
  `reindex-search` rebuild.
- Var-value flattening is byte-stable across platforms and runs for the
  fixture corpus (same determinism bar as AN.4 renders).
- Decomposed rows contribute no `body_text` and plain rows contribute no
  `var_values` (scope check per Decision 10).
- The public search capability passes its contract suite without SQLite; the
  SQLite implementation returns the same typed result semantics in parity
  fixtures.

## Required validation

- index-consistency property tests
- backfill vs reindex equivalence tests
- cross-platform flattening determinism test
- cargo test/format/lint suite

## Non-closure

No query API, no CLI, no HTTP surface — AN.6 consumes these indexes. Whether
FTS results are reachable by consumers is not testable until AN.6; this
sprint's closure is index correctness only, and it claims nothing further.
