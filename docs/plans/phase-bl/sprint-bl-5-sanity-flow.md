# Sprint bl-5 — sanity state transitions and mechanical D5 selection

`BOUNDARY-SanityState`; wave 2; must follow bl-1's `StateSnapshot` and
`EventRequest` types plus bl-2 JSONL. Store Jev/LLM records unchanged,
evaluate D5 from current state, and emit an `EventRequest`; bl-3 later
debounces and delivers that shared type. It never couples evaluation to
collection timing or interprets verdict prose.

> "if jev-api failed (i.e. jev did not respond), the parallel llm evaluator's response is used. If both llm+jev respond a failure on either is failure on iteration #1. for iteration > 1, sanity-agent to triage (which should be done w/ background agent if sanity-agent has queue backlog)."

Controlled fixtures prove D5, preserved history, transition-to-request, and
absence of an ATM call before scheduled delivery.

## Relations, scope, and owned paths

Own dev-sanity templates, sanity record/history/merge scripts, assignment
gates, examples, and their tests. It consumes bl-2 JSONL and bl-1 types; it
does not own task-close/stack mechanics or debounce/delivery implementation.

## Public contract and deliverables

`select_iteration(iteration, deliverable_id, jev_lines, llm)` returns a
selected evaluation, failure, or triage route. The record preserves request
arrays and ordered JSONL unchanged, validates closed error kinds, and sums
only numeric elapsed/retry fields. D5 transitions emit `EventRequest` values;
they never send them.

## Deterministic acceptance and validation

- Fixtures prove LLM fallback, iteration-one failure, later triage, preserved
  JSONL, old-record readability, and one emitted request per eligible state
  transition.
- Run plugin manifest checks and focused pytest; never use timing/load tests.
