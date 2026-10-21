# Sprint bl-12 — collector/state/event pipeline integration

Wave 3 integration; must follow bl-3 event requests, bl-4 reviewer metadata,
bl-5 sanity state, and bl-7 display. Controlled inputs prove Jev JSONL and
reviewers enter current persisted state, the TUI displays it, eligible
transitions create debounced aggregated event requests, and a separately
advanced delivery clock sends them. Cron reports the same state when it does
not collect; failed sources are eligible next request. No product path changes.

## Relations, scope, and owned paths

Own new pipeline integration fixtures/tests only. It consumes bl-3 delivery
history, bl-4 reviewers, bl-5 sanity state and bl-7 display; it edits none of
their producer paths. `must_follow`: bl-3, bl-4, bl-5, and bl-7 artifacts;
`parallel_safe`: none before those integration inputs are stable.

## Governing text (verbatim)

> "collect-data (cron and/or tui) and save to db -> process data (determine state) and save to db -> fire events (after debouncing & aggregation)"

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

## Authoritative deliverables and deterministic acceptance

For every Jev error kind, controlled JSONL flows through sanity state to the
do-not-enter display; reviewer metadata reaches the Quality pane unchanged.
Controlled transitions create aggregated/debounced requests and only an
advanced delivery clock sends them. Cron/TUI equality and failed-source retry
are asserted from persisted records. Run deterministic fixture tests and lint.

- [ ] `test_each_jev_error_kind_flows_jsonl_to_sanity_and_do_not_enter`: parameterized controlled JSONL fixtures preserve every error kind through state into the display contract.
- [ ] `test_reviewer_metadata_reaches_quality_pane_unchanged`: an injected reviewer fixture reaches the Quality pane without transformation.
- [ ] `test_eligible_sanity_transition_creates_aggregated_request`: controlled state input creates one debounced request through the bl-3 seam.
- [ ] `test_only_advanced_delivery_clock_sends_request`: delivery fake is untouched before its injected schedule becomes due.
- [ ] `test_cron_and_tui_render_same_persisted_state_without_collection`: shared fixture records produce identical views and zero extra reads.
- [ ] `test_failed_source_is_eligible_on_next_controlled_request`: a supplied failed record is retried under the bl-1 success-stamp rule.

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pytest -q
cd plugins/atm-bd-orchestration && python3 -m pytest -q -rs
```

No integration test uses live services, sleeps, races, event ordering, or load.

## Non-closure

This adds fixture-level proof only. It does not change Jev clients, reviewer producers, sanity selection, collector policy, dispatch behavior, or console implementation paths.
