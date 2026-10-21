# Sprint bl-12 — Phase BL composition

closure_type: integration; target_boundary: PhaseBlComposition; branch: sprint/bl-12-phase-composition; PR target: integrate/phase-bl; wave: 3; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1, bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14; parallel_safe: none.

Own `cron_entry.py`, `delivery_entry.py`, `wiring.py`, and `test_phase_bl_composition.py` in the console package. This is the only feature-level composition owner; it changes no producer implementation path.

## Governing text (verbatim)

> "collect-data (cron and/or tui) and save to db -> process data (determine state) and save to db -> fire events (after debouncing & aggregation)"

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

## Deliverables

1. Wire the fixed architecture: collectors → persisted JSON state → state stage → `EventRequest` → debounce/aggregate → separately scheduled delivery, with no database.
2. Wire both cron-originated and bl-5 sanity-originated `EventRequest`s through the named `enqueue_event_request` seam into bl-3's durable queue; `delivery_entry.deliver_due(clock)` is the only send entry.
3. Compose JSONL error state, reviewer metadata, persisted snapshot, Quality presentation, event aggregation, and delivery history using deterministic fakes.

## Deterministic acceptance

- [ ] `test_each_jev_error_kind_flows_jsonl_to_sanity_and_do_not_enter`
- [ ] `test_reviewer_metadata_reaches_quality_pane_unchanged`
- [ ] `test_eligible_sanity_transition_creates_one_aggregated_request`
- [ ] `test_eligible_sanity_transition_creates_aggregated_request`
- [ ] `test_sanity_event_request_enters_durable_queue_via_enqueue_event_request`
- [ ] `test_only_advanced_delivery_clock_sends_request`
- [ ] `test_cron_and_tui_render_same_persisted_state_without_collection`
- [ ] `test_failed_source_is_eligible_on_next_controlled_request`

All fixtures inject the source, stored JSON, queue, ATM sender, and clock. No integration test sleeps, races, orders event loops, loads CPU, or contacts a service.

## This sprint does not close

It proves composition only; individual source reads, derivation, queue semantics, display rendering, and sanity selection remain owned by their boundary sprints.
