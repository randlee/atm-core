# Sprint bl-5 — sanity state and close mechanics

closure_type: boundary; target_boundary: SanityState; branch: sprint/bl-5-sanity-flow; PR target: integrate/phase-bl; wave: 2; external_ref: #365, #497, #126, #498; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`EventRequest` JSON codec); parallel_safe: bl-2, bl-3, bl-4, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14.

Own `agents/dev-sanity.md`, `templates/dev-sanity-template.xml.j2`, `templates/sanity-run-record.json.j2`, `scripts/{sanity-merge,sanity-run-history,sanity-stack-link,atm-task-close.py}`, and `scripts/tests/test_sanity_*.py` under the plugin. Every listed transition, request array, record, and close/stack rule is production-ready at this boundary.

## Deliverables

1. Pack context for the LLM and write the `JevRequest[]` array into `dev-sanity.md`; persist the exact JSONL/result array in the sanity-run log with typed error context (#365, #497).
2. Select deterministic sanity transitions and emit a contract-codec `EventRequest` through the named `enqueue_event_request` seam; bl-12 proves its arrival in bl-3's durable queue.
3. Fold bl-11 mechanics here: `sanity-stack-link` records the stack/rebase link, and `atm-task-close.py` makes a not-reproducible close explicit, auditable, and linked (#498, #126).
4. Apply D5 mechanically: `select_iteration(iteration, deliverable_id, jev_lines, llm) -> SelectedEvaluation | IterationFailure | TriageRoute`. Jev unavailable uses LLM; at iteration 1 either negative response fails; later negative routes sanity-agent triage. Records retain ordered arrays, reject unknown error kinds, and sum only numeric elapsed/retry values.

> "if jev-api failed (i.e. jev did not respond), the parallel llm evaluator's response is used. If both llm+jev respond a failure on either is failure on iteration #1. for iteration > 1, sanity-agent to triage (which should be done w/ background agent if sanity-agent has queue backlog)."

## Deterministic acceptance

- [ ] `test_sanity_log_preserves_jev_request_array_and_jsonl`
- [ ] `test_context_packing_writes_jev_request_array_to_dev_sanity`
- [ ] `test_sanity_transition_emits_contract_event_request_to_enqueue_seam`
- [ ] `test_not_reproducible_close_and_rebase_link`
- [ ] `test_atm_task_close_records_stack_link_before_terminal_close`
- [ ] `test_jev_unavailable_uses_llm_evaluation`
- [ ] `test_jev_negative_and_llm_negative_fail_iteration_one`
- [ ] `test_later_negative_routes_to_triage`
- [ ] `test_d5_selection_is_mechanical_from_current_state`
- [ ] `test_record_round_trips_ordered_arrays_and_closed_error_kinds`
- [ ] `test_numeric_elapsed_and_retry_summing_ignores_non_numeric_fields`

Tests use saved records and a fake enqueue/close seam; no live daemon, sleep, or race.

## Validation
```bash
cd plugins/atm-bd-orchestration && python3 tests/gen_manifest.py --check && python3 -m pytest -q -rs
```

## This sprint does not close

It does not implement durable queue/delivery (bl-3) or prove cross-boundary arrival (bl-12).
