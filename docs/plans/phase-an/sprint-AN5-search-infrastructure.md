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
section; FTS external-content drift risk entry; ADR-018 §3 and ADR-036's
Phase AN extension (the approved fourth/fifth optional storage capabilities).
Requirement IDs assigned during plan hardening.

## Deliverables

1. Message FTS index (FTS5) covering, per row: `message_text` (plain and
   file-ref rows only), `summary`, flattened tags, and — for decomposed rows
   — flattened **var values** from `vars_json`:

```sql
CREATE TABLE IF NOT EXISTS mail_message_search_documents (
    search_rowid INTEGER PRIMARY KEY,
    team TEXT NOT NULL,
    agent TEXT NOT NULL,
    message_key TEXT NOT NULL,
    message_id TEXT NULL,
    message_at TEXT NOT NULL,
    body_text TEXT NOT NULL,   -- plain/file-ref text; '' for decomposed
    summary TEXT NOT NULL DEFAULT '',
    tags TEXT NOT NULL DEFAULT '',
    var_values TEXT NOT NULL DEFAULT '',
    from_agent TEXT NOT NULL,
    UNIQUE(team, agent, message_key)
);
CREATE VIRTUAL TABLE IF NOT EXISTS mail_messages_fts USING fts5(
    body_text, summary, tags, var_values, from_agent,
    content='mail_message_search_documents',
    content_rowid='search_rowid',
    tokenize='unicode61 remove_diacritics 2'
);
```

   This is external-content FTS, not a contentless table: each FTS row maps
   through `search_rowid` to the exact compound mailbox key. A live SQLite
   spike is required before DDL freeze and must assert
   `snippet(mail_messages_fts, ...)` returns highlighted text (the verified
   reference result for `alpha beta gamma MATCH beta` is `alpha [beta] gamma`).
   Var-value flattening is deterministic: values in key-sorted order, scalar
   values verbatim, arrays flattened in order, objects recursed in key order.
2. Template-content FTS uses the same external-content pattern over a
   `message_template_search_documents(search_rowid INTEGER PRIMARY KEY,
   template_sha TEXT UNIQUE NOT NULL, content_text TEXT NOT NULL)` projection
   and `message_templates_fts`; it indexes AN.2's UTF-8 `content_text`, never
   the raw BLOB. Boilerplate queries join by `template_sha` to messages, and
   template snippets use the projection (consumed by AN.6).
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
   SQLite parity and index-consistency tests. This same PR adds the
   `atm-storage` and `atm-storage-rusqlite` boundary manifests/inventory
   entries and their authorized implementation/test-double records.

```rust
pub trait MessageSearchStore: sealed::Sealed + Send + Sync {
    fn search(&self, query: &MessageSearchQuery)
        -> Result<MessageSearchPage, AtmError>;
}

pub enum SearchExpression {
    Atom(SearchAtom),
    All(Vec<SearchExpression>),
    Any(Vec<SearchExpression>),
    Not(Box<SearchExpression>),
    Near { terms: Vec<SearchAtom>, max_distance: u8 },
}

pub enum SearchAtom {
    Term(String),
    Phrase(String),
}

#[async_trait::async_trait]
pub trait AsyncMessageSearchStore: MessageSearchStore {
    async fn search_async(
        &self,
        query: MessageSearchQuery,
        deadline: SearchDeadline,
    ) -> Result<MessageSearchPage, AtmError>;
}

pub struct MessageSearchQuery {
    pub expression: Option<SearchExpression>,
    pub filters: SearchFilters,
    pub aggregate: Option<SimpleAggregate>,
    pub page: SearchPageRequest,
    pub per_mailbox: bool,
}

pub struct SearchFilters {
    pub team: Option<TeamName>,
    pub agent: Option<AgentName>,
    pub from_agent: Option<AgentName>,
    pub template_sha: Option<TemplateSha>,
    pub template_metadata: Vec<(SearchKey, SearchValue)>,
    pub vars: Vec<(SearchKey, SearchValue)>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub time_range: Option<TimeRange>,
}

pub enum SimpleAggregate {
    Count,
    GroupBy(SearchGroupBy),
    Min(SearchTimestampField),
    Max(SearchTimestampField),
}

pub struct MessageSearchPage {
    pub matches: Vec<StoredSearchMatch>,
    pub aggregate: Option<SearchAggregate>,
    pub next_cursor: Option<SearchCursor>,
}

pub struct StoredSearchMatch {
    pub key: SearchResultKey,
    pub message_id: Option<String>,
    pub message_at: IsoTimestamp,
    pub from: StoredSearchAddress,
    pub to: StoredSearchAddress,
    pub template_sha: Option<TemplateSha>,
    pub template_type: Option<String>,
    pub category: Option<String>,
    pub match_fields: Vec<SearchMatchField>,
}

pub struct StoredSearchAddress {
    pub agent: AgentName,
    pub team: TeamName,
    pub chat_id: Option<ChatId>,
}

pub struct SearchKey(String);   // validated public key grammar
pub struct SearchValue(String); // data value, never SQL/FTS syntax

pub struct SearchLimit(u32); // validated 1..=200; default is 50
pub struct SearchCursor(String); // opaque encoding of the final stable sort tuple
pub struct SearchPageRequest {
    pub limit: SearchLimit,
    pub cursor: Option<SearchCursor>,
}
pub struct SearchDeadline { pub remaining: Duration }

pub struct SearchResultKey {
    pub team: TeamName,
    pub agent: AgentName,
    pub message_key: MessageKey,
}

pub enum SearchMatchField {
    BodyText,
    Summary,
    Tag,
    VarValue,
    TemplateContent,
}

pub enum SearchGroupBy {
    Field(SearchGroupField),
    Var(SearchKey),
}

pub enum SearchGroupField {
    Team,
    Agent,
    FromAgent,
    TemplateType,
    Category,
}

pub enum SearchTimestampField { MessageAt }

pub struct TimeRange {
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
}

pub struct SearchGroup {
    pub key: String,
    pub count: u64,
}

pub enum SearchAggregate {
    Count { value: u64 },
    Groups { by: SearchGroupBy, groups: Vec<SearchGroup> },
    Timestamp { field: SearchTimestampField, value: Option<IsoTimestamp> },
}
```

   All types in this block are canonical leaf `atm-storage` DTOs; AN.6 imports
   them and defines no competing aggregate/group shape. A match contains
   durable message projection fields and match provenance, never a rendered
   snippet or a renderer handle; `atm-core` turns it into a presentation hit
   and invokes its `TemplateComposer` port only when a decomposed snippet is
   needed. CLI and HTTP both decode into the core request, core validates/maps
   it one-to-one to `MessageSearchQuery`, and only the SQLite adapter compiles
   that query to FTS5/JSON1. The authorized in-memory fake is recorded in the
   storage boundary manifest and runs the same AST/filter/aggregate contract
   without SQLite. `AsyncMessageSearchStore` is the Tokio-safe companion of
   the one search capability, not a separate semantic capability trait. The
   rusqlite backend owns a bounded reader executor and observes
   `SearchDeadline`/future cancellation; `atm-http-runtime` only awaits this
   port and must not issue direct SQLite reads or `spawn_blocking`.

   AN.5 freezes the validation and semantic rules consumed by AN.6:

   - `SearchKey` is ASCII `^[A-Za-z_][A-Za-z0-9_-]{0,63}$`; dots, brackets,
     quotes, NUL, and path-shaped keys are rejected before JSON1 path build.
     `SearchValue` is bounded data (maximum 4 KiB), never FTS/SQL syntax.
   - `SearchLimit` is required after CLI/HTTP defaulting (`50`, inclusive
     range `1..=200`); a zero, overflow, or out-of-range value is typed input
     failure. `TimeRange` accepts either endpoint but rejects `since > until`.
   - Results sort by `(message_at DESC, team ASC, agent ASC, message_key ASC)`.
     `SearchCursor` encodes that complete final tuple and resumes strictly
     after it; malformed/mismatched cursors are typed input failure.
   - `--per-mailbox` uses `SearchResultKey(team, agent, message_key)` exactly.
     Default dedup uses non-NULL `message_id`; a NULL ID falls back to that
     same compound key. The first record in the stable sort wins every tie.
   - The parser bounds AST depth (`8`), node count (`64`), atom bytes (`256`),
     and rejects empty boolean groups and every unrecognized token. Normal
     positional input becomes exactly one `Phrase`; only `--raw-match` builds
     composition nodes. `Not` is legal only inside `All` with at least one
     positive sibling and
     subtracts its child match set from that sibling/filter candidate set;
     standalone `Not` and `Any(Not(...))` are rejected. `Near` accepts
     `2..=8` atoms and distance `1..=16`; all atoms must occur unordered in
     the same indexed field, within that token-gap distance. It never spans
     FTS columns or message/template document projections.

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
- FTS external-content tests prove `snippet()`/`highlight()` return a
  non-empty highlighted fragment and every hit maps to its compound
  `SearchResultKey`; no nullable `message_id` is used as physical identity.
- Async search parity runs through the backend-owned reader lane under a
  deadline/cancellation fixture; HTTP adapters contain neither `spawn_blocking`
  nor direct SQLite reader work.

## Required validation

- index-consistency property tests
- backfill vs reindex equivalence tests
- cross-platform flattening determinism test
- live SQLite external-content/snippet DDL spike plus migration test
- async reader-lane deadline/cancellation and fake/SQLite parity suite
- cargo test/format/lint suite

## Non-closure

No query API, no CLI, no HTTP surface — AN.6 consumes these indexes. Whether
FTS results are reachable by consumers is not testable until AN.6; this
sprint's closure is index correctness only, and it claims nothing further.
