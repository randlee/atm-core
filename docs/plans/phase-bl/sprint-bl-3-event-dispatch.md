# Sprint bl-3 — event dispatch
Implementation repository/worktree: `randlee/atm-bd-orchestration`; branch/PR target: `integrate/phase-bl`.

closure_type: boundary; target_boundary: EventDispatch; branch: sprint/bl-3-event-dispatch; PR target: integrate/phase-bl; wave: 2; external_ref: #491, #359; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`EventRequest`, `DeliveryHistoryEntry` codec); parallel_safe: bl-2, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13.

Own `events.py`, `delivery.py`, `test_events.py`, and `test_delivery.py` in the console package. It implements durable enqueue, debounce/aggregation, due selection, separately scheduled delivery, and the production ATM sender schedule/recipient selection; it does not collect or render. The durable v1 queue and history live at the configured console state path `events.v1.jsonl`, with one versioned request/history record per line. This is the sole enqueue CLI contract owner: `enqueue-event` returns a versioned typed acknowledgement or typed rejection.

## Deliverables

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

> "multiple collectors each in separate module governed by it's own rules (i.e. gh rate limited, dolt timeouts, atm no limits today). state evaluation based on current state. specific state transitions would trigger 'atm message request'."

1. Implement pure `derive_transitions(snapshot, previous) -> tuple[EventRequest, ...]` for held, dispatch, and alert transitions. Retained held rows carry bead, task, reason, and source timestamp.
2. Persist and enqueue derived or supplied `EventRequest`s through `enqueue_event_request`; aggregate/debounce by `debounce_key` and preserve delivery history. `enqueue-event --request <json>` is owned here and returns exactly `{version,status,request_id,error}` with `status` `acknowledged` or `rejected`; neither bl-5 nor bl-12 defines that CLI behaviour.
3. Implement `due_requests(clock)` and `deliver_due(clock, sender)`; only `deliver_due` may invoke sender.
4. Persist request identity and attempt state before send. A failed typed sender result remains due for controlled-clock retry; restart recovery reloads it; acknowledged success is persisted before it becomes ineligible for redelivery.
5. Schedule production ATM delivery from persisted due requests only, using the request recipients, and record each typed outcome. The proposed 60-second delivery cadence/debounce remains a documented proposal pending Rand confirmation; no sender runs on cron/TUI collection.

## Deterministic acceptance

- [ ] `test_enqueue_persists_event_request_and_debounce_key`
- [ ] `test_enqueue_cli_returns_versioned_acknowledgement_or_rejection`
- [ ] `test_derive_transitions_emits_held_dispatch_and_alert_requests`
- [ ] `test_held_row_preserves_bead_task_reason_and_source_timestamp`
- [ ] `test_debounce_aggregates_requests_deterministically`
- [ ] `test_due_requests_never_call_send_seam`
- [ ] `test_deliver_due_is_only_send_seam_and_respects_injected_clock`
- [ ] `test_delivery_history_preserves_typed_outcome`
- [ ] `test_failed_send_remains_due_for_controlled_clock_retry`
- [ ] `test_restart_recovers_pending_delivery_request`
- [ ] `test_acknowledged_success_is_never_redelivered`
- [ ] `test_due_delivery_uses_persisted_recipients_and_controlled_schedule`

No source-read or TUI assertion is made here; bl-12 owns collector/TUI no-send integration proof.

## Validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q tests/test_events.py tests/test_delivery.py
```
