---
title: Phase AN Plan — Decomposed Template Messages and a Template-Agnostic Query Surface
status: draft (planning in progress — see Open questions)
branch: plan/phase-an
baseline: integrate/phase-al @ 0ef581e1 (assumes Phase AM deletion completes first; see Entry gate)
---

# Phase AN — Template + Vars Storage, Queryable Message Database

## Why this phase exists (query requirements first)

The motivating requirement is **extracting operational truth from the message
corpus**, independent of how agents are orchestrated. Representative queries
the system must make possible (expressed today against the current
orchestration's templates, but not limited to them):

- Q1. The time span of every sprint (first assignment → completion message).
- Q2. How many QA iterations occurred per sprint.
- Q3. How many findings — Blocking / Important / Minor — each QA round produced.
- Q4. Which agent did dev work on each sprint.
- Q5. Ad hoc: find messages by template type, var value, sender, time window,
  or free text.

**Current state:** this information exists in sc-compose template inputs,
gets rendered into prose, written to tmp files, and agents write ad hoc
Python parsers over those files to detect when the team goes off the rails.
Structured data is round-tripped through prose and regex. Measured message
sizes (share-dir scan 2026-08: avg 4.11 KB, max 35 KB) confirm content
volume is trivial — the problem is structure loss, not size.

**Core invariant — template-agnostic queryability:** different
orchestrations bring different templates; messages must *still* be
queryable. atm therefore ships **mechanism, not vocabulary**: it stores
template identity + input variables and exposes generic introspection,
filtering, and aggregation. It never reserves key names, never knows what a
"sprint" is, and never binds to the currently-active workflow. Because
template rows are immutable and content-addressed, messages from retired
orchestrations remain renderable and queryable forever. Q1–Q4 are validation
stories proving the generic surface, not features of atm core.

## Decisions (settled with product owner, 2026-08-10)

1. **Decomposed storage for templated messages.** A templated message stores
   `template_sha` + fully **merged** vars JSON (post-precedence resolution of
   `--var` > `--var-file` > `--env-prefix` > `input_defaults` > frontmatter
   defaults; env-sourced values captured at compose time). Body renders for
   the reader at read time. The stored `vars_json` is byte-for-byte the
   structure today's Python parsers try to reconstruct.
2. **Renderer is `sc-composer` (library crate), embedded in-process through
   one dedicated adapter.** Jinja2 body + YAML frontmatter
   (`required_variables`/`defaults`/`metadata`); the adapter is the documented
   embedding path. No shell-out and no direct `sc-composer` dependency from
   the storage contract, HTTP runtime, or CLI.
3. **SHA over the full template file**, algorithm exactly matching
   synaptic-canvas-dolt via sc-compose's implementation — never
   reimplemented in atm. Purpose: tamper-evidence for prompts/templates.
4. **Open admission.** On-the-fly prompt edits are legitimate until
   synaptic-canvas-dolt is fully running; unknown templates register on
   first use, SHA recorded for later audit. Enforcement is a later phase.
5. **No cross-host decomposed messages.** Non-same-team/repo recipients get
   sender-rendered plain text. No template sync protocol in AN.
6. **CLI:** `atm send <agent> --template <path> --vars <file.json>` =
   compose + register + send. `atm compose` is a thin passthrough to
   sc-compose (preview/validate), no mailbox interaction.
7. **No reserved var vocabulary in atm.** Key semantics belong to the
   templates and the skills/orchestrations that own them. atm's query
   surface treats all keys uniformly; the frontmatter schema stored per
   template is the self-describing contract consumers introspect.
8. **Include graphs remain out of scope** (their pinning and portable
   reproduction belong to sc-compose/dolt). Until that layer provides an
   immutable include graph, detected include directives take the safe
   `WARN + rendered-plain-text fallback`: ATM does not register a template or
   admit a decomposed row, and sends the one verified rendered body as an
   ordinary plain message. This keeps a same-host/local include from becoming
   a non-reproducible stored reference and preserves the SHA evidence for the
   raw input without pretending the include graph is durable.
9. **No transport cap raise** (measured sizes make it unnecessary).
10. **NULL body for decomposed rows** (resolved 2026-08-10). `message_text`
    is NULL for decomposed messages; the body exists only by rendering.
    Free-text coverage is recovered structurally: FTS indexes flattened
    **var values** + `summary` + tags per message, and template boilerplate
    is searched at the *template* layer (FTS over
    `message_templates.content`, expanded to messages via `template_sha`) —
    honest semantics, since a boilerplate phrase occurs in every instance of
    its template. Phrase queries spanning the boilerplate/var boundary are
    accepted as out of scope. Historical migrated DBs already hold
    `message_text` as nullable (SQLite `ADD COLUMN` cannot retrofit
    NOT NULL); fresh DDL relaxes to match, and read paths handle the option
    as part of AN.3's render-on-read work. Search-result snippets for
    decomposed hits render on demand or synthesize from vars (AN.4).
11. **Search-key stratification** (resolved 2026-08-10). The three template
    columns serve three search roles: `template_sha` is revision identity
    (tamper-evidence and exact-render reproducibility — not a search key);
    template frontmatter metadata (`type` and any other keys) is the
    **stable search dimension across derivative revisions** — a minor
    template edit mints a new SHA but inherits the frontmatter, so
    derivatives land in the same bucket (`dev-fix`, `qa-report`, …) and
    searchers filtering by type + vars (`repo`, `branch`, `phase`,
    `sprint`) are unaffected by syntax churn; `vars_json` is instance data.
    atm tracks no template lineage (no parent-sha, no versioning) — the
    frontmatter traveling with the file does the bucketing;
    synaptic-canvas-dolt owns true lineage. The query surface exposes a
    generic `--template-meta key=value` filter over stored frontmatter
    metadata (with `--type` as sugar for the conventional key), so future
    metadata keys become search dimensions with no atm changes.
12. **The conventional template-type key is `metadata.type`.** The catalog
    records that exact key as `template_type`; it accepts no aliases or
    fallback taxonomy. A template without `metadata.type` is admissible with
    `template_type = NULL` and emits a registration WARN, making the missing
    stable search dimension visible without ATM inventing workflow vocabulary.
13. **Search is host-local.** `GET /v1/atm/messages/search` is available through
    authenticated local UDS/loopback adapters only. The direct-peer listener
    may register the canonical route inventory, but the core application
    policy rejects a `SearchRequest` with peer ingress before any storage
    call. Remote query authorization, mailbox scoping, and result redaction
    are explicitly out of scope for AN.

## Design principles

- **SQLite is durable truth; shared Claude JSON gains no new fields** (per
  `docs/atm-message-schema.md` §4 / Phase Y ledger rule). JSONL export
  renders decomposed bodies then applies existing
  `claude_jsonl_body_export_max_bytes` stub rules unchanged.
- **Additive migrations only** (existing `ALTER TABLE … ADD COLUMN`
  pattern).
- **Never touch legacy daemon code** (AL/AM boundary rules).
- **Determinism is a tested invariant:** `render(template_sha, merged_vars)`
  byte-identical across hosts/platforms/time; violations are blocking
  findings.
- **Queries derive from stored schema, not from atm code.** Anything a
  consumer needs to construct a query must be discoverable via
  introspection at runtime.
- **Storage capabilities stay separable.** Template persistence/catalog and
  message search are narrow, backend-neutral storage capabilities, rather
  than methods accreted onto the existing mailbox store. SQLite/FTS/JSON1 are
  an adapter implementation detail, not a public query contract.

## Architectural ownership and reusable seams

Phase AN preserves the current dependency direction and makes its two new
capabilities independently reusable:

| Layer | Owns | Must not own |
|---|---|---|
| `atm-storage` | Leaf DTOs (`TemplateSha`, template records, decomposed body, typed search filters/aggregates/results) and sealed `TemplateCatalogStore` / `MessageSearchStore` capabilities | SQLite, FTS syntax, JSON1 paths, HTTP, `sc-composer`, orchestration vocabulary |
| `atm-storage-rusqlite` | Additive migrations, atomic template/message admission, FTS5/JSON1 compilation, backfill/reindex, implementations of the storage capabilities | Public query grammar or renderer behavior |
| dedicated `sc-composer` adapter crate | Raw-byte hash, frontmatter extraction, variable resolution, rendering, and one translation of upstream diagnostics | Storage access, HTTP/CLI parsing, routing policy |
| `atm-core` | Application policy, the renderer port, transport-neutral search request/outcome mapping, and canonical HTTP codec/route registration | SQLite/FTS/JSON1 or direct `sc-composer` calls |
| `atm` and `atm-http-runtime` | CLI/HTTP adaptation to the same core contract | Domain query semantics, SQL compilation, rendering implementation |
| `atm-daemon-bootstrap` replacement composition, via `atm-runtime` assembly | Wiring the storage and `sc-composer` adapters into core through the one approved backend-neutral assembly input | Legacy synchronous-daemon behavior, a second persistence/runtime service |

`MessageSearchStore` is a separate sealed capability, not a catch-all
extension of `MessageStore`. Its interface accepts immutable typed query and
aggregate DTOs and returns typed pages/results; it never accepts SQL, raw FTS
syntax, `rusqlite` values, HTTP DTOs, or renderer handles. This keeps AN's
database search easy to lift into a future framework extension after a second
consumer proves the generalization, without prematurely creating a framework
in this phase. Contract tests use a fake/in-memory implementation; SQLite
parity and index-consistency tests remain adapter tests. Any new authorized
adapter implementation follows ADR-001's sealed-trait and boundary-lint
process.

The existing `--raw-match` spelling does **not** pass SQLite FTS5 syntax to
the storage capability. It selects ATM's documented advanced-search grammar,
which core parses into a bounded `SearchExpression` AST (terms, phrases,
boolean composition, and proximity). `MessageSearchStore` receives that AST;
the SQLite adapter compiles it privately. Unsupported syntax is a typed parse
error, never a backend fallback or raw SQL/FTS payload.

## Data model

### `message_templates` (new table)

```sql
CREATE TABLE IF NOT EXISTS message_templates (
    template_sha TEXT NOT NULL PRIMARY KEY,   -- dolt-compatible SHA, full file
    template_type TEXT NULL,                   -- from frontmatter metadata
    template_name TEXT NULL,
    content TEXT NOT NULL,                     -- full file, immutable
    schema_json TEXT NOT NULL DEFAULT '{}',    -- extracted frontmatter
    first_seen_at TEXT NOT NULL,
    first_seen_by TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_message_templates_type
    ON message_templates(template_type) WHERE template_type IS NOT NULL;
```

Rows immutable, content-addressed; re-registration of a known SHA is a
no-op.

### `mail_messages` additions (additive migration)

```sql
ALTER TABLE mail_messages ADD COLUMN template_sha TEXT NULL;
ALTER TABLE mail_messages ADD COLUMN vars_json TEXT NULL;      -- fully merged
ALTER TABLE mail_messages ADD COLUMN category TEXT NULL;
ALTER TABLE mail_messages ADD COLUMN content_format TEXT NULL;
ALTER TABLE mail_messages ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';
```

Message source union, discriminated by column presence: plain
(inline/stdin), decomposed (`template_sha` + `vars_json` set), file-ref
(legacy `--file`). FK to `message_templates` enforced in the storage layer
(same transaction as admission), keeping the migration additive.
Classification fields (`category`, `tags`, `content_format`) apply to all
sources; for templated messages `template_type` is the primary classifier.
For decomposed rows, `message_text` is NULL as settled in Decision 10; no
implementation question remains.

## Query surface (core deliverable)

Three layers, all template-agnostic:

1. **Introspection.** `atm templates list [--type T]` and
   `atm templates schema <sha>` return registered templates and their
   frontmatter-derived schemas from `schema_json`. This is how any consumer
   discovers what is queryable — atm compiles in nothing. A type is a
   non-unique discovery filter only: `list --type` returns every matching
   revision; schema lookup always takes an exact immutable SHA.
2. **Generic query primitives.** `atm search` (CLI) and
   `GET /v1/atm/messages/search`
   (HTTP): filters on team/agent/sender/time/`template_sha`/`category`/tag,
   `--template-meta key=value` over stored frontmatter metadata (`--type`
   is sugar for the conventional key; prefix matching supported), repeatable
   `--var key=value` filters over `vars_json` (JSON1), free text via FTS5,
   dedup by `message_id`. Simple aggregations (`--count`,
   `--group-by <var|field>`, `--min/--max message_at`) cover
   span/count/rollup questions over CLI and HTTP. Full aggregation
   expressiveness is intentionally NOT built over HTTP in AN — see layer 3.
3. **Stable SQL view contract for local consumers.** A documented, versioned
   read-only view — working name `decomposed_messages(team, agent,
   from_agent, message_at, message_id, template_type, template_sha,
   vars_json, category, tags_json, read/ack state…)` — is the public
   contract for skills/agents that outgrow the CLI (today's Python-parser
   authors). Underlying tables stay free to evolve; the view does not.
   An agent's "parser" becomes a SQL query against the view.

The CLI and HTTP surface map into the same typed core request, which in turn
uses `MessageSearchStore`; neither surface reaches SQLite or compiles query
syntax. The storage capability may use FTS5 and JSON1 internally, but its
public contract is intentionally backend-neutral.

The HTTP route is local-only per Decision 13. Core checks authenticated ingress
before dispatching a search, so the peer listener cannot use the route to
enumerate local durable message history.

FTS5 (per Decision 10): the message index covers `message_text` (plain and
file-ref rows), `summary`, flattened tags, and **flattened var values** for
decomposed rows; a second index covers `message_templates.content` so
boilerplate text is searched at the template layer and expanded to messages
via `template_sha`. FTS5 availability in bundled rusqlite is an AN.1 gate
test.

## Send/read flow

**Send (templated):** resolve full variable precedence via `sc-composer`
(validation failures surface sc-compose diagnostics as typed atm errors),
compute dolt-compatible SHA, render once to verify → same-team: register
template if unknown, store decomposed; otherwise store/send rendered plain
text. **Read:** read/peek/list/JSONL-export detect `template_sha`, render
with stored merged vars. Render failure at read is treated as corruption
(typed error naming `message_id` + `template_sha`; recoverable by template
re-registration). **`atm compose`:** passthrough preview so agents stop
hand-rendering to files.

## Sprints

Sprint docs are authoritative for deliverables, acceptance criteria, and
required validation (per `.claude/skills/plan-hardening/`
`sprint-planning-guidelines.md`); this section is the dependency map only.
The earlier 6-sprint outline was re-split to 8 under the split-early rule so
each sprint owns a single closure type, and send/read/index work can run in
parallel once the schema lands.

| Sprint | Doc | must_follow | parallel_safe |
|---|---|---|---|
| AN.1 contract gates (sc-composer pin, dolt SHA oracle, FTS5 gate, fixture capture) | [sprint-AN1](./sprint-AN1-contract-gates.md) | none (AM-ledger checked) | late AM |
| AN.2 storage schema (template store, message columns, view v1) | [sprint-AN2](./sprint-AN2-storage-schema.md) | AN.1 | none |
| AN.3 send surface (templated send, merged vars, routing) | [sprint-AN3](./sprint-AN3-send-surface.md) | AN.2 | AN.4, AN.5 |
| AN.4 render-on-read (read paths, export, determinism CI) | [sprint-AN4](./sprint-AN4-render-on-read.md) | AN.2 | AN.3, AN.5 |
| AN.5 search infrastructure (FTS indexes + sync) | [sprint-AN5](./sprint-AN5-search-infrastructure.md) | AN.2 | AN.3, AN.4 |
| AN.6 query surface (introspection, search CLI, HTTP) | [sprint-AN6](./sprint-AN6-query-surface.md) | AN.5, AN.4, AN.2 | AN.7 |
| AN.7 compose passthrough, guidance, telemetry | [sprint-AN7](./sprint-AN7-compose-guidance.md) | AN.3 | AN.4, AN.5, AN.6 |
| AN.8 validation and evidence (Q1-Q4, parser replacement, agnosticism check) | [sprint-AN8](./sprint-AN8-validation-evidence.md) | AN.3, AN.4, AN.6, AN.7 | none |

Merge-forward rule (repo convention): `must_follow` children merge the
parent's pushed integration line before every dev/fix round.

Entry gates carried by sprints: AN.2 consumes the settled `metadata.type`
catalog rule in Decision 12; AN.3 requires Open question 4 resolved; AN.6
requires Open question 5 confirmed.

## Open questions and resolved gates (do not implement past an unresolved gate)

1. ~~Decomposed rows and `message_text NOT NULL`~~ — **resolved as
   Decision 10** (NULL body; FTS over var values + summary + tags;
   template-content search with sha-join expansion; cross-boundary phrase
   queries out of scope).
2. ~~Include-directive containment~~ — **resolved as Decision 8**: detect
   includes, WARN, and send only the verified rendered plain-text fallback;
   do not register/admit decomposed content until sc-compose/dolt provides a
   pinned include graph.
3. ~~Frontmatter key for `template_type`~~ — **resolved as Decision 12**:
   exact key `metadata.type`; absent key is admitted as NULL with a
   registration WARN and no aliases.
4. **New `MAX_STDIN_MESSAGE_BYTES`:** proposal config-driven, 1 MiB default
   (aligns with transport cap). Plain sends only.
5. **HTTP query scope:** proposed split — filters + simple aggregates over
   HTTP; anything gnarlier runs host-local via the SQL view. Confirm.
6. ~~`session_id` filtering~~ — deferred to AH.1. It is not durable on the
   phase-al line, AN owns no migration for it, and no AN filter or endpoint
   may imply session-scoped query support.

## Non-goals (explicitly deferred)

- Reserved/workflow var vocabularies, sprint semantics, or any
  orchestration-aware reporting in atm core (orchestration layer owns these).
- Template allowlist enforcement against synaptic-canvas-dolt; cross-host
  template sync; include-graph pinning (sc-compose/dolt layer).
- Full aggregation language over HTTP; semantic/vector search; daemon
  auto-classification.
- Historical backfill: prose/XML-envelope messages predating templated sends
  are not parsed into vars — metrics accrue forward-only.
- Transport cap raise; attachment store; hard rejection of path-only bodies.

## Risks

- **SHA drift vs dolt (version/input skew, not algorithm choice):** atm
  depends on sc-compose's hash implementation precisely because
  reimplementing would be riskier. Residual risk: (a) version skew — atm's
  pinned `sc-composer` and dolt's deployment diverge across a hash-affecting
  change (line-ending/BOM/trailing-newline normalization), invalidating
  historical SHAs; (b) call-boundary skew — same function, different bytes
  fed (raw-on-disk vs transformed read, CRLF checkouts). Both silent.
  Mitigation: golden vectors pinned against dolt's *recorded output*,
  re-run on every `sc-composer` bump; hash-is-public-API confirmed in AN.1.
- **Renderer non-determinism:** platform/locale/time-varying Jinja2
  behavior breaks render-on-read equality — cross-platform byte-equality CI
  (AN.3); instances are blocking findings.
- **View-contract erosion:** consumers writing SQL against underlying
  tables instead of the versioned view recreates the parser problem one
  layer down — mitigated by documenting the view as the only supported
  surface and keeping table names out of consumer-facing docs.
- **Vars quality:** queryability is only as good as what template authors
  promote into variables; prose-embedded facts stay unqueryable (modulo
  Open question 1 FTS scope). Owned at the template/dolt layer; atm's
  introspection at least makes the gap visible.
- **Template/DB referential integrity:** storage-layer FK bug could orphan
  `template_sha` refs — same-transaction registration + corruption-path
  recovery.
- **FTS external-content drift:** write paths bypassing triggers — property
  tests + `reindex-search`.
- **AM interleaving:** AN touches no legacy files; AN.1 file lists checked
  against the frozen AM removal ledger.
