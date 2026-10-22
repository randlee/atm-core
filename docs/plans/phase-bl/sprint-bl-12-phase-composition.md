# Sprint bl-12 — Phase BL composition
Implementation repository/worktree: `randlee/atm-bd-orchestration`; branch/PR target: `integrate/phase-bl`.

closure_type: integration; target_boundary: PhaseBlComposition; branch: sprint/bl-12-phase-composition; PR target: integrate/phase-bl; wave: 3; external_ref: none; recommended_agent: trinity; recommended_model: opus; must_follow: bl-1, bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13; parallel_safe: none.

Own `cli.py`, `app.py`, `runner.py`, `model.py`, `cron_entry.py`, `delivery_entry.py`, `wiring.py`, `test_phase_bl_composition.py`, and `test_app.py` in the console package. It replaces the legacy bundled `runner.step` entry path: installed cron, show, and TUI call collector/state/event/delivery seams independently.

## Governing text (verbatim)

> "collect-data (cron and/or tui) and save to db -> process data (determine state) and save to db -> fire events (after debouncing & aggregation)"

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

## Deliverables

1. Wire the fixed architecture: collectors → persisted JSON state → state stage → `EventRequest` → debounce/aggregate → separately scheduled delivery, with no database. `cron_entry` calls bl-3 `derive_transitions(snapshot, previous)` then `enqueue_event_request`. This is the sole owner that populates `StateSnapshot.held_rows`, `qa`, `jev`, and `dag` from their versioned fixtures during snapshot composition.
2. Wire both cron-originated and bl-5 sanity-originated `EventRequest`s through the named `enqueue_event_request` seam into bl-3's durable queue; `delivery_entry.deliver_due(clock)` is the only send entry.
3. Compose JSONL error state, reviewer metadata, persisted snapshot, Quality presentation, event aggregation, and delivery history using deterministic fakes.
4. Production wiring injects bl-10 `ProblemDeriver` into bl-8 `build_snapshot` and routes bl-5's JSON/CLI enqueue seam into bl-3.
5. Cut over installed `cli.py`, `app.py`, `runner.py`, and `model.py`: no collection entrypoint calls sender, and TUI/cron share persisted snapshots without a second source query.
6. Fold the former ConsoleApp boundary here: one console instance, persisted-state consumption, `r` force request, PR-tab GitHub-only request, and Rich Diagnostics colour.

## Deterministic acceptance

- [ ] `test_each_jev_error_kind_flows_jsonl_to_sanity_and_do_not_enter`
- [ ] `test_reviewer_metadata_reaches_quality_pane_unchanged`
- [ ] `test_eligible_sanity_transition_creates_one_aggregated_request`
- [ ] `test_sanity_event_request_enters_durable_queue_via_enqueue_event_request`
- [ ] `test_cron_enqueues_transition_derived_from_fixture_snapshot`
- [ ] `test_composition_populates_held_rows_qa_jev_and_dag_from_v1_fixtures`
- [ ] `test_wiring_injects_problem_deriver_and_plugin_enqueue_path`
- [ ] `test_installed_cron_show_and_tui_entrypoints_never_send_or_double_query`
- [ ] `test_composed_quality_two_panel_markdown_selection`
- [ ] `test_composed_failed_send_retries_after_restart_without_redelivery_after_ack`
- [ ] `test_second_console_exits`
- [ ] `test_refresh_key_calls_collector_request_seam_with_force_true_for_every_source`
- [ ] `test_pr_tab_activation_requests_github_only`
- [ ] `test_diagnostics_rich_text_colour_is_preserved`
- [ ] `test_console_never_sends_atm_message`
- [ ] `test_only_advanced_delivery_clock_sends_request`
- [ ] `test_cron_and_tui_render_same_persisted_state_without_collection`
- [ ] `test_failed_source_is_eligible_on_next_controlled_request`

All fixtures inject the source, stored JSON, queue, ATM sender, and clock. No integration test sleeps, races, orders event loops, loads CPU, or contacts a service.

## Validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
cd plugins/atm-bd-orchestration && python3 -m pytest -q -rs
```

## This sprint does not close

It proves composition only; individual source reads, derivation, queue semantics, display rendering, and sanity selection remain owned by their boundary sprints.
