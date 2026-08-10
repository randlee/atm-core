---
title: AN.6 Query Surface — Introspection, Search CLI, HTTP Endpoint
status: draft
branch: feature/pan-s6-query-surface
worktree: ../atm-core-worktrees/feature/pan-s6-query-surface
target: integrate/phase-an
---

# AN.6 — Query Surface: Introspection, Search CLI, HTTP Endpoint

**recommended_agent:** arch-ctm/deep-reasoning (query compilation and the
public contract freeze).
**must_follow:** AN.5 (indexes), AN.4 (sole renderer port), and AN.2 (view);
merge all pushed integration lines before each dev or fix round.
**unblocks:** AN.8.
**parallel_safe:** AN.7 (non-intersecting: AN.7 owns the compose passthrough
command, send-admission lint, and docs; this sprint owns search/templates
commands and the HTTP route).

**traceability:** plan-phase-an.md Decisions 7, 10, 11; Query surface
section (three layers); Open question 5 (HTTP scope — must be confirmed
before this sprint starts; treated as entry gate). Requirement IDs assigned
during plan hardening.

## Deliverables

1. Introspection commands — the mechanism that keeps atm template-agnostic:
   `atm templates list [--type T] [--json]` and
   `atm templates schema <sha> [--json]`, serving `template_sha`,
   `template_type`, `template_name`, `first_seen_*`, and the stored
   `schema_json`. Introspection output must be sufficient to construct every
   query this sprint's acceptance uses, without reading atm source. `--type`
   is deliberately a list filter (and can match multiple revisions); schema
   lookup is exact-SHA only, so no result is selected arbitrarily during
   template drift.
2. `atm search` with one filter grammar:
   free-text positional argument (sanitized literal text; `--raw-match`
   selects the documented ATM advanced-search grammar parsed by core into a
   backend-neutral `SearchExpression`, never raw FTS5 syntax),
   `--template-meta key=value` over stored frontmatter
   metadata (`--type` as sugar for the conventional key; prefix matching),
   repeatable `--var key=value` (JSON1 over `vars_json`), `--tag`,
   `--category`, `--from`, `--team`, `--since`, `--until`, `--limit`,
   `--json`, `--per-mailbox` (default dedups by `message_id`).
   Free text searches `body_text`/`summary`/`tags`/`var_values` per AN.5
   scope, and template boilerplate via the template index with SHA-join
   expansion.
3. Simple aggregations over the same filter grammar: `--count`,
   `--group-by <var:KEY|field>`, `--min message_at`, `--max message_at`.
   Nothing richer lands over CLI or HTTP in AN.
4. Define the transport-neutral search request/response mapping and canonical
   HTTP codec/route registration in `atm-core`, then expose
   `GET /v1/atm/messages/search` through `atm-http-runtime` as a thin adapter.
   CLI and HTTP use the same
   core contract, which maps to the backend-neutral `MessageSearchStore`
   capability introduced by AN.5. `atm-http-runtime` owns no search DTO,
   query semantics, rendering, or storage dependency. The shared typed
   request is:

```rust
pub struct SearchRequest {
    pub text: Option<SearchExpression>,
    pub template_meta: Vec<(String, String)>,
    pub vars: Vec<(String, String)>,
    pub tags: Vec<String>,
    pub category: Option<String>,
    pub from_agent: Option<String>,
    pub team: Option<String>,
    pub since: Option<IsoTimestamp>,
    pub until: Option<IsoTimestamp>,
    pub limit: Option<u32>,
    pub per_mailbox: bool,
    pub aggregate: Option<SimpleAggregate>,
}

pub enum SimpleAggregate {
    Count,
    GroupBy(SearchGroupBy),
    Min(SearchTimestampField),
    Max(SearchTimestampField),
}

pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub aggregate: Option<SearchAggregate>,
}

pub struct SearchHit {
    pub message_id: MessageId,
    pub message_at: IsoTimestamp,
    pub from_agent: AgentAddress,
    pub to_agent: AgentAddress,
    pub template_type: Option<String>,
    pub category: Option<String>,
    pub snippet: String,
}

pub enum SearchAggregate {
    Count(u64),
    Groups(Vec<SearchGroup>),
    Timestamp(IsoTimestamp),
}
```

   The route is local-only: core rejects `AuthenticatedIngress::Peer` with a
   documented typed authorization error before selecting a storage capability.
   It does not create peer query authorization, remote mailbox selection, or
   result-redaction behavior in AN. Update the maintained route table in
   `docs/atm-daemon/http-api.md`, the checked-in
   `docs/atm-http-runtime/openapi.yaml` path `/messages/search`, and
   route-surface/additions-only snapshots in the same PR.

5. Search-result rendering: hit lines carry `message_id`, timestamp,
   from→to, template type or category, and a snippet (FTS `snippet()` for
   indexed text; on-demand render prefix for decomposed body context).
6. Contract freeze: `docs/atm-query-surface.md` finalized — the
   `decomposed_messages` view (AN.2), the filter grammar, and the HTTP
   contract are versioned public surfaces from this sprint onward.

## Acceptance criteria

- Seeded corpus is retrievable by template metadata, var value, tag,
  category, sender, time window, and free text (including a
  boilerplate-phrase query resolved via the template index) over both CLI
  and HTTP, with identical result sets.
- **Derivative-revision test:** a trivially edited template (new SHA, same
  frontmatter) is registered; messages sent under both revisions are
  returned together by `--type`/`--template-meta` and `--var` filters.
- Aggregations return correct counts, group rollups, and min/max spans
  against hand-computed fixture expectations.
- Injection suite: quotes, `NEAR`, boolean operators, and column-filter
  syntax in the free-text argument cannot alter query semantics without
  `--raw-match`.
- Query-key boundary suite: metadata, variable, and group-by keys use one
  explicit public key grammar and parameterized JSON1-path construction;
  malformed or traversal-shaped keys are rejected identically over CLI and
  HTTP and cannot select a different JSON path.
- The advanced-search grammar is parsed to the same bounded
  backend-neutral `SearchExpression` for CLI and HTTP; arbitrary FTS5 syntax
  is rejected before `MessageSearchStore` is called.
- UDS and capability-authenticated loopback search succeed for a seeded local
  mailbox; the direct-peer listener receives the documented local-only error
  and returns no result, including for a forged team/agent filter.
- Every acceptance query was constructed from introspection output alone
  (demonstrated in the test fixtures' comments).

## Required validation

- corpus fixture tests over CLI and HTTP asserting parity
- FTS injection suite
- query-key boundary suite over CLI and HTTP
- advanced-search parser/AST parity and rejection suite
- local-vs-peer search ingress authorization suite
- aggregation correctness fixtures
- contract snapshot test for `GET /v1/atm/messages/search` request/response
  serialization, including every `SimpleAggregate` request form and every
  `SearchAggregate` response form
- boundary test proving the HTTP runtime only adapts the core search contract
  and that CLI and HTTP compile to the same typed request
- cargo test/format/lint suite

## Non-closure

Full aggregation expressiveness over HTTP is deferred (view is the local
escape hatch). Q1–Q4 validation stories and the parser-replacement test are
AN.8, not this sprint.
