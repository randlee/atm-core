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
`must_follow`: bl-1 types and bl-2 JSONL. `parallel_safe`: bl-3, bl-6, and bl-7 after those inputs are available.

Owned filenames: `agents/dev-sanity.md`,
`templates/dev-sanity-template.xml.j2`, `templates/sanity-run-record.json.j2`,
`scripts/sanity-merge`, `scripts/sanity-run-history`,
`scripts/assignment-gates.py`, and their focused `scripts/tests/` cases.

## Public contract and deliverables

`select_iteration(*, iteration: int, deliverable_id: str, jev_lines: Sequence[JevLine], llm: LlmReply) -> SelectedEvaluation | IterationFailure | TriageRoute`
returns a selected evaluation, failure, or triage route. The record preserves request
arrays and ordered JSONL unchanged, validates closed error kinds, and sums
only numeric elapsed/retry fields. D5 transitions emit `EventRequest` values;
they never send them.

## Deterministic acceptance and validation

- [ ] `test_jev_unavailable_uses_llm_evaluation`: a Jev transport-failure fixture selects the parallel LLM result without changing stored JSONL.
- [ ] `test_jev_negative_and_llm_negative_fail_iteration_one`: controlled responses prove either negative verdict fails iteration one.
- [ ] `test_later_negative_routes_to_triage`: the same controlled negative result above iteration one selects triage rather than rewriting history.
- [ ] `test_d5_selection_is_mechanical_from_current_state`: fixtures cover the D5 decision table, including selected evaluation and route.
- [ ] `test_record_round_trips_ordered_arrays_and_closed_error_kinds`: old and new JSONL fixtures preserve request arrays and reject unknown error kinds.
- [ ] `test_eligible_transition_emits_one_event_request_without_send`: a deterministic state change emits one typed request and invokes no ATM seam.

```bash
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```

Fixtures, not clock timing or load, drive every case.

## Non-closure

Does not collect Jev/LLM data, debounce/deliver messages, alter task-close or stack mechanics, or reinterpret verdict prose. It stores supplied records and selects the documented state transition only.
