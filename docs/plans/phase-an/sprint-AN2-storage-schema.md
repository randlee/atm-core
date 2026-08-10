---
title: AN.2 Storage Schema — Template Store, Message Columns, View v1
status: draft
branch: feature/pan-s2-storage-schema
worktree: ../atm-core-worktrees/feature/pan-s2-storage-schema
target: integrate/phase-an
---

# AN.2 — Storage Schema: Template Store, Message Columns, View v1

**recommended_agent:** arch-ctm/deep-reasoning (schema/migration correctness
across historical databases).
**must_follow:** AN.1; merge AN.1's pushed integration line before each dev or
fix round (consumes `TemplateSha`, `TemplateFrontmatter`).
**unblocks:** AN.3, AN.4, AN.5.
**parallel_safe:** none; this sprint owns the storage-schema boundary.

**traceability:** plan-phase-an.md Decisions 1, 10, 11; Data model section;
`docs/atm-message-schema.md` §4 (no new shared-inbox fields). Requirement IDs
assigned during plan hardening.

**Entry gate:** Decision 12 is fixed before this sprint starts: catalog
`template_type` means the literal frontmatter path `metadata.type`, with no
aliasing or inferred taxonomy. A missing key stores `NULL` and is admissible;
the registration WARN is implemented by AN.3.

## Deliverables

1. `message_templates` table exactly as specified in plan-phase-an.md (SHA
   primary key, `template_type`, `template_name`, immutable `content`,
   `schema_json`, `first_seen_at`, `first_seen_by`, partial type index).
   Re-registration of a known SHA is a no-op.
2. Additive `mail_messages` migration: `template_sha`, `vars_json`,
   `category`, `content_format`, `tags_json DEFAULT '[]'`; fresh DDL relaxes
   `message_text` to `NULL` (historical migrated DBs already hold it
   nullable — parity, not a data migration).
3. Storage-layer message-source union and its column mapping:

```rust
pub enum MessageBody {
    Inline(String),                                        // message_text set
    Decomposed { template_sha: TemplateSha, vars: MergedVars }, // text NULL
    FileRef(String),                                       // reference text
}
```

   Write invariants, enforced in the storage layer in the **same
   transaction**: `Decomposed` requires an existing `message_templates` row
   (registering it if new); `Inline`/`FileRef` require `template_sha` and
   `vars_json` NULL; no other column combinations are representable.
   `MessageBody`, `TemplateSha`, stored-template records, and the registration
   request/result are leaf `atm-storage` DTOs. A narrow sealed
   `TemplateCatalogStore` capability owns registration/load semantics; the
   SQLite adapter implements it and the atomic decomposed-message admission
   without exposing a connection or transaction to callers. This same PR adds
   the `atm-storage` and `atm-storage-rusqlite` boundary manifests/inventory
   entries and their authorized implementation/test-double records.
4. `decomposed_messages` view v1 — the versioned public contract for local
   SQL consumers:

```sql
CREATE VIEW IF NOT EXISTS decomposed_messages AS
SELECT m.team, m.agent, m.from_agent, m.message_at, m.message_id,
       m.template_sha, t.template_type, m.vars_json,
       m.category, m.tags_json, m.summary,
       s.read, s.acknowledged_at, s.pending_ack_at
FROM mail_messages m
JOIN message_templates t ON t.template_sha = m.template_sha
LEFT JOIN mail_message_states s
  ON (s.team, s.agent, s.message_key) = (m.team, m.agent, m.message_key)
WHERE m.template_sha IS NOT NULL;
```

   Documented in a new `docs/atm-query-surface.md` page: the view (not the
   underlying tables) is the supported consumer surface; changes are
   versioned.
5. Migration/equivalence test fixtures: historical-DB fixture and fresh-DB
   fixture produce identical query-visible state; orphaned `template_sha`
   writes fail atomically.
6. Shared-inbox guard: snapshot test proving none of the new fields appear in
   ATM-authored shared Claude JSON output.

## Acceptance criteria

- Migrated historical fixture DBs and fresh DBs are query-equivalent.
- A `Decomposed` write without a resolvable template fails with no partial
  row; template registration and message admission commit atomically.
- The view's column set matches `docs/atm-query-surface.md` exactly.
- The shared-inbox guard test passes; no Phase-Y ledger entry is required
  because no shared-surface field is added.
- The template catalog contract is usable by a fake/in-memory implementation
  with no SQLite or renderer dependency; SQLite-specific atomicity remains
  covered by this sprint's adapter tests.

## Required validation

- storage round-trip and property tests (insert/update/delete across all
  three `MessageBody` variants)
- historical-DB migration fixture suite
- shared Claude JSON export snapshot guard
- cargo test/format/lint suite

## Non-closure

No send or read behavior changes; FTS tables and triggers are AN.5; no CLI
flags land here. The view is created but its contract freeze happens in AN.6
after query-surface consumers exercise it.
