---
title: Phase AN Plan — Decomposed Template Messages and a Template-Agnostic Query Surface
status: AN.1–AN.10 complete; AN.11–AN.12 workflow-metadata extension planned; AN.13–AN.15 blocked on upstream sc-compose 1.4.1 release gates
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
5. **No cross-host decomposed messages.** Only a same-team **and same-host**
   recipient can receive a decomposed row. Any cross-host recipient,
   including a same-team recipient on another host, receives sender-rendered
   plain text. No template sync protocol exists in AN.
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
   ordinary plain message, but only after the pinned upstream resolver proves
   every resolved target is under the declared template root (no absolute
   paths or symlink escape). A target that cannot be so contained fails closed
   without rendering or admission. This keeps a same-host/local include from
   becoming a non-reproducible stored reference and preserves the SHA evidence
   for the raw input without pretending the include graph is durable.
9. **No transport cap raise** (measured sizes make it unnecessary).
10. **NULL body for decomposed rows** (resolved 2026-08-10). `message_text`
    is NULL for decomposed messages; the body exists only by rendering.
    Free-text coverage is recovered structurally: FTS indexes flattened
    **var values** + `summary` + tags per message, and template boilerplate
    is searched at the *template* layer (FTS over
    `message_templates.content_text`, expanded to messages via `template_sha`) —
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
| `atm-storage` | Leaf DTOs (`TemplateSha`, `MergedVarsJson`, template records, decomposed admission, typed async search filters/aggregates/pages) and sealed catalog/search capabilities | SQLite, FTS syntax, JSON1 paths, HTTP, `sc-composer`, orchestration vocabulary |
| `atm-storage-rusqlite` | Additive migrations, atomic template/message admission, bounded async reader lane, FTS5/JSON1 compilation, backfill/reindex, implementations of the storage capabilities | Public query grammar, renderer behavior, or request-worker blocking |
| `atm-template-sc-compose` adapter crate | Raw-byte hash, frontmatter extraction, include-reference detection, variable resolution, rendering, and one translation of upstream diagnostics | Storage access, HTTP/CLI parsing, routing policy |
| `atm-core` | Application policy, the renderer port, transport-neutral search request/outcome mapping, and canonical HTTP codec/route registration | SQLite/FTS/JSON1 or direct `sc-composer` calls |
| `atm` and `atm-http-runtime` | CLI/HTTP adaptation to the same core contract | Domain query semantics, SQL compilation, rendering implementation |
| `atm-daemon-bootstrap` replacement composition, via `atm-runtime` assembly | Wiring the storage and `atm-template-sc-compose` adapters into core through the one approved backend-neutral assembly input | Legacy synchronous-daemon behavior, a second persistence/runtime service |

`MessageSearchStore` is a separate sealed capability, not a catch-all
extension of `MessageStore`. Its interface accepts immutable typed query and
aggregate DTOs and returns typed pages/results; it never accepts SQL, raw FTS
syntax, `rusqlite` values, HTTP DTOs, or renderer handles. This keeps AN's
database search easy to lift into a future framework extension after a second
consumer proves the generalization, without prematurely creating a framework
in this phase. Contract tests use a fake/in-memory implementation; SQLite
parity and index-consistency tests remain adapter tests. Its async companion
is the same semantic capability's Tokio-safe read lane, not an additional
capability trait; the concrete backend owns queueing, deadlines, cancellation,
and any synchronous SQLite reader. Any new authorized adapter implementation
follows ADR-001's sealed-trait and boundary-lint process.

The existing `--raw-match` spelling does **not** pass SQLite FTS5 syntax to
the storage capability. It selects ATM's documented advanced-search grammar,
which core parses into a bounded `SearchExpression` AST (terms, phrases,
boolean composition, and proximity). `MessageSearchStore` receives that AST;
the SQLite adapter compiles it privately. Unsupported syntax is a typed parse
error, never a backend fallback or raw SQL/FTS payload.

The public AST is deliberately small and backend-neutral. A normal positional
search becomes one `Phrase`; only `--raw-match` admits the composition nodes.
Leaf text is data, not a query fragment: core validates it for non-emptiness,
length, and the documented character limits before the adapter compiles it.
`SearchExpression` and `SearchAtom` are leaf `atm-storage` DTOs; their exact
enum shape, key/limit/time/cursor validation, and `Not`/`Near` semantics are
frozen by AN.5 before AN.6 begins. Core owns parsing and mapping, but storage
never depends on core. `atm-storage-rusqlite` compiles only those bounded
variants with bound values; it has no raw-FTS escape hatch, and the fake
storage contract tests execute the same AST semantics without SQLite.

## Data model

### `message_templates` (new table)

```sql
CREATE TABLE IF NOT EXISTS message_templates (
    template_sha TEXT NOT NULL PRIMARY KEY,   -- dolt-compatible SHA, full file
    template_type TEXT NULL,                   -- from frontmatter metadata
    template_name TEXT NULL,
    content_bytes BLOB NOT NULL,               -- raw file, immutable/hash input
    content_text TEXT NOT NULL,                -- strict UTF-8 projection, FTS only
    schema_json TEXT NOT NULL DEFAULT '{}',    -- extracted frontmatter
    first_seen_at TEXT NOT NULL,
    first_seen_by TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_message_templates_type
    ON message_templates(template_type) WHERE template_type IS NOT NULL;
```

Rows are immutable and content-addressed; re-registration of a known SHA is
a no-op. Admission persists and reloads `content_bytes` byte-for-byte. It
also requires a strict UTF-8 decode and stores that exact projection as
`content_text` for the template FTS index; invalid UTF-8 fails before any
catalog/message write with a typed template-content error.

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
   `--var key=value` filters over `vars_json` (JSON1), free text via FTS5.
   The frozen AN.5 page contract defines stable sort, opaque cursor, and
   dedup: default uses non-NULL `message_id` with compound-key fallback;
   `--per-mailbox` uses the exact `(team, agent, message_key)` key. Simple
   aggregations (`--count`,
   `--group-by <var|field>`, `--min/--max message_at`) cover
   span/count/rollup questions over CLI and HTTP. Full aggregation
   expressiveness is intentionally NOT built over HTTP in AN — see layer 3.
3. **Stable SQL view contract for local consumers.** A documented, versioned
   read-only view — working name `decomposed_messages(team, agent,
   from_agent, message_at, message_id, template_type, template_sha,
   vars_json, category, tags_json, read/ack state…)` — is the public
   contract for skills/agents that outgrow the CLI (today's Python-parser
   authors). Underlying tables stay free to evolve; the view does not.
   AN.6 also ships the local-only `atm_query` Maturin extension (crate
   `atm-query-python`) for these consumers: `open_readonly(...).query(sql,
   parameters)` runs one parameterized raw read-only SQLite statement and
   returns Python rows. This deliberately permits an analyst to change a
   detailed query without changing Rust, while keeping that capability out of
   the CLI, HTTP API, peer listener, and generic `MessageSearchStore`.
   `decomposed_messages` is the supported stable raw-query surface; callers
   may inspect underlying tables at their own compatibility risk.

   The extension opens the configured database read-only, sets `query_only`
   and defensive SQLite connection settings, rejects a multi-statement tail,
   and requires SQLite's prepared-statement read-only classification before
   execution. Its authorizer denies writes, schema changes, transactions,
   `ATTACH`/`DETACH`, unsafe pragmas, and extension loading. It additionally
   applies a bounded execution deadline, result-row cap, and result-byte cap.
   These are defense in depth for a trusted local analytical tool, not a
   remote authorization mechanism. The Maturin boundary lives beside the
   rusqlite adapter, not in `atm-core` or `atm-graft-python`, because raw SQL
   is necessarily SQLite-specific and must not contaminate the backend-neutral
   storage capability.

The CLI and HTTP surface map into the same typed core request, which in turn
uses `MessageSearchStore`; neither surface reaches SQLite or compiles query
syntax. The storage capability may use FTS5 and JSON1 internally, but its
public contract is intentionally backend-neutral.

The HTTP route is local-only per Decision 13. Core checks authenticated ingress
before dispatching a search, so the peer listener cannot use the route to
enumerate local durable message history.

FTS5 (per Decision 10): the message index covers `message_text` (plain and
file-ref rows), `summary`, flattened tags, and **flattened var values** for
decomposed rows; a second index covers `message_templates.content_text` so
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
| AN.9 template workflow contract and migration | [sprint-AN9](./sprint-AN9-template-workflow-contract.md) | AN.8 merged | none |
| AN.10 atomic workflow admission snapshots and tag provenance | [sprint-AN10](./sprint-AN10-template-workflow-admission.md) | AN.9 merged | none |
| AN.11 local workflow analytics/query and optional telemetry projection | [sprint-AN11](./sprint-AN11-workflow-analytics-projection.md) | AN.10 merged | none |
| AN.12 workflow-metadata validation and retained evidence | [sprint-AN12](./sprint-AN12-workflow-validation-evidence.md) | AN.11 merged | none |
| AN.13 checked-render catalog-format contract | [sprint-AN13](./sprint-AN13-sc-compose-141-checked-render.md) | AN.10 merged | none |
| AN.14 checked-render runtime upgrade to sc-compose 1.4.1 | [sprint-AN14](./sprint-AN14-sc-compose-141-checked-emission.md) | AN.13 merged | none |
| AN.15 adversarial checked-render assurance | [sprint-AN15](./sprint-AN15-adversarial-fuzzing.md) | AN.14 merged | none |

Merge-forward rule (repo convention): `must_follow` children merge the
parent's pushed integration line before every dev/fix round.

**Shared external gate for AN.13–AN.15:** crates.io publishes `sc-sha`
**1.4.1**, `sc-composer` **1.4.1**, and `sc-compose` **1.4.1**; the published
`sc-composer` release exports `check_rendered_output`, `CheckedOutput`, and
`OutputFormat`; and [sc-compose #448](https://github.com/randlee/sc-compose/issues/448)
is closed with its direct-library regression. `sc-compose`, like
`sc-sha`/`sc-composer`, is a crates.io package (the present 1.4.0 release
establishes that publication model). This is an external release gate, not a
`must_follow` relation; its exact wording is authoritative for all three
sprints.

Entry gates carried by sprints: AN.2 consumes the settled `metadata.type`
catalog rule in Decision 12; AN.3 consumes the resolved message-size policy;
AN.6 consumes the resolved HTTP and Python query-surface policies.

## AN extension — template-declared workflow facts

AN.1–AN.10 completed the generic template catalog, decomposed admission,
read/query surface, and atomic workflow snapshot/tag-provenance capture.
AN.11–AN.12 extend that completed substrate with local analytics and retained
validation evidence. The extension is governed by
[ADR-046](../../adr/ADR-046-template-declared-workflow-metadata.md) and the
author-facing [template workflow metadata guide](../../template-workflow-metadata.md).

The extension preserves the phase's template-agnostic invariant. ATM validates
and stores opaque workflow declarations; it does not define a development,
review, or incident process. Template tags are copied to a durable admission
snapshot so changing a template revision cannot rewrite historical facts. The
existing `tags_json` remains caller/instance data, while applied-template tags
and derived effective tags have explicit, separate provenance.

The extension deliberately uses four sequential closure types: public
contract/schema (AN.9), atomic runtime behavior (AN.10), local analytical
projection (AN.11), then whole-line evidence (AN.12). This prevents a
schema-only or query-only PR from claiming workflow analytics are complete.

AN.13–AN.15 are a separate, post-AN.10 adapter/storage hardening track. AN.13
owns the durable output-format contract required for an honest render-on-read
decision; AN.14 then consumes the published `sc-composer` 1.4.1 checked-render
contract on every production emission route. This split prevents a schema-only
PR from claiming malformed JSON is rejected. The track is intentionally not
part of the workflow-metadata extension: output-format identity and checked
rendering are generic template concerns, not workflow vocabulary.

AN.15 is a test-and-evidence closure sprint after the runtime change, not a
policy engine. It extends the existing bounded adversarial-fuzz campaign
contract to exercise ATM's template adapter/catalog routes. It proves that ATM
preserves immutable input facts (raw revision SHA, parsed metadata, captured
variables, and output format) without guessing a template's approval lineage.
Repository-specific approval, protected-frontmatter, expected-tag, and
lineage lint rules remain owned by the repository that supplies templates.

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
4. ~~**New `MAX_STDIN_MESSAGE_BYTES`**~~ — **resolved as Decision 14**:
   replace the current hard-coded 256 KiB inline/stdin cap with one
   configuration-backed `max_message_bytes`, default 1 MiB, for every plain
   message admission path. This is a message policy rather than a SQLite row
   limit: SQLite can store far larger values, but ATM must still bound
   buffering and peer work. The HTTP body limit must be configured as
   `max_message_bytes + documented canonical-envelope overhead`, so a valid
   maximum-size message is not rejected merely because JSON framing adds
   bytes. Tests cover inline, stdin, UDS, and TCP/HTTP equality at the limit
   and one byte over; no distinct lower stdin limit is permitted.
5. ~~**HTTP query scope**~~ — **resolved as Decision 15**: CLI and local-only
   HTTP expose the same bounded typed filter grammar and simple aggregates:
   team, agent, sender, time range, document type (`metadata.type`),
   template SHA, category, tag, stored variable, and FTS text; plus count,
   allowlisted grouping, and min/max timestamp. HTTP accepts neither raw SQL,
   raw FTS syntax, arbitrary expressions, joins, nor views. Detailed and
   evolving analyst queries run locally through AN.6's Maturin
   `atm_query` read-only interface against `decomposed_messages`, with SQL
   parameters and SQLite-enforced read-only execution. This supports queries
   such as all assignments across several development agents, those limited
   to a phase and document type, and all fix assignments or QA findings for a
   phase, without a Rust change per query.
6. ~~`session_id` filtering~~ — deferred to AH.1. It is not durable on the
   phase-al line, AN owns no migration for it, and no AN filter or endpoint
   may imply session-scoped query support.

## Non-goals (explicitly deferred)

- Reserved/workflow var vocabularies, sprint semantics, or any
  orchestration-aware reporting in atm core (orchestration layer owns these).
- Template allowlist enforcement against synaptic-canvas-dolt; cross-host
  template sync; include-graph pinning (sc-compose/dolt layer).
- Full aggregation language over HTTP; semantic/vector search; daemon
  auto-classification. The local `atm_query` Maturin extension is the explicit
  raw read-only analytical escape hatch, not an expansion of either public
  transport API.
- Historical backfill: prose/XML-envelope messages predating templated sends
  are not parsed into vars — metrics accrue forward-only.
- The optional workflow declaration extension does not add an ATM-owned
  workflow vocabulary, remote analytics endpoint, cross-host template sync,
  or an admission/routing dependency on telemetry.
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
