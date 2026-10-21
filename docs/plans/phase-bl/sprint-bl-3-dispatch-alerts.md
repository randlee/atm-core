# Sprint bl-3 — event dispatch

closure_type: boundary; target_boundary: EventDispatch; branch: sprint/bl-3-event-dispatch; PR target: sprint/bl-1-phase-contract; wave: 2; external_ref: #491, #359; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`EventRequest`, `DeliveryHistoryEntry` codec); parallel_safe: bl-2, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13, bl-14.

Own `events.py`, `delivery.py`, `test_events.py`, and `test_delivery.py` in the console package. It implements durable enqueue, debounce/aggregation, due selection, and separately scheduled delivery; it does not collect or render.

## Deliverables

1. Persist and enqueue `EventRequest`s through `enqueue_event_request`; aggregate/debounce by `debounce_key` and preserve delivery history.
2. Implement `due_requests(clock)` and `deliver_due(clock, sender)`; only `deliver_due` may invoke sender.

## Deterministic acceptance

- [ ] `test_enqueue_persists_event_request_and_debounce_key`
- [ ] `test_debounce_aggregates_requests_deterministically`
- [ ] `test_due_requests_never_call_send_seam`
- [ ] `test_deliver_due_is_only_send_seam_and_respects_injected_clock`
- [ ] `test_delivery_history_preserves_typed_outcome`

No source-read or TUI assertion is made here; bl-12 owns collector/TUI no-send integration proof.
