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
**must_follow:** AN.5 (indexes) and AN.2 (view); merge both pushed
integration lines before each dev or fix round.
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
   `atm templates schema <sha|type> [--json]`, serving `template_sha`,
   `template_type`, `template_name`, `first_seen_*`, and the stored
   `schema_json`. Introspection output must be sufficient to construct every
   query this sprint's acceptance uses, without reading atm source.
2. `atm search` with one filter grammar:
   free-text positional argument (sanitized FTS MATCH; raw syntax only behind
   `--raw-match`), `--template-meta key=value` over stored frontmatter
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
4. `GET /v1/search` on `atm-http-runtime` exposing exactly the same
   filter/simple-aggregate contract as one typed request/response pair:

```rust
pub struct SearchRequest {
    pub text: Option<String>,
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
    pub aggregate: Option<SimpleAggregate>, // Count | GroupBy(..) | MinMax(..)
}
```

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
- Every acceptance query was constructed from introspection output alone
  (demonstrated in the test fixtures' comments).

## Required validation

- corpus fixture tests over CLI and HTTP asserting parity
- FTS injection suite
- aggregation correctness fixtures
- contract snapshot test for `GET /v1/search` request/response serialization
- cargo test/format/lint suite

## Non-closure

Full aggregation expressiveness over HTTP is deferred (view is the local
escape hatch). Q1–Q4 validation stories and the parser-replacement test are
AN.8, not this sprint.
