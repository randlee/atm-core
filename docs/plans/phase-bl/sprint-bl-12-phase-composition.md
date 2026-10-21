# Sprint bl-12 — phase BL composition

closure_type: integration; target_boundary: PhaseBlComposition; branch:
sprint/bl-12-phase-composition; PR target: integrate/phase-bl; wave 3;
external_ref: #491,#359; recommended_agent: arch-ctm; recommended_model: opus.
must_follow: bl-2,3,4,5,6,7,8,10,13,14. parallel_safe: none.

Own cron_entry.py, delivery_entry.py, wiring.py and test_phase_bl_composition.py.
Cron runs collectors, build_snapshot, and enqueue; separately scheduled
delivery calls deliver_due. This only sprint claims feature-level cron/TUI
equality, failed-source retry, or no sends from collector/TUI paths.

- [ ] test_cron_and_tui_render_same_persisted_state_without_collection
- [ ] test_failed_source_is_eligible_on_next_controlled_request
- [ ] test_collectors_and_tui_never_call_delivery_send
- [ ] test_real_cron_builds_snapshot_and_enqueues_transition
- [ ] test_only_scheduled_delivery_entry_calls_deliver_due

Every listed deliverable lands production-ready through real composition using
only clock, ATM, and source fakes.
