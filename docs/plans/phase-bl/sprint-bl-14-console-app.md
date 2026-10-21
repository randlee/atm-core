# Sprint bl-14 — console app collection controls

closure_type: boundary; target_boundary: ConsoleApp; branch: sprint/bl-14-console-app; PR target: integrate/phase-bl; wave: 2; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`CollectorRequest`); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-13.

Own `app.py` and `test_app.py`. Implement the console-instance lock, persisted-state consumption, PR-tab GitHub refresh request, and Rich Diagnostics colour; never call a delivery seam from this boundary.

## Deliverables

1. Enforce one console instance and consume persisted snapshot state only.
2. Route `r` to the contract request seam and PR activation to GitHub only; the collector implementation defines force/lock semantics.
3. Preserve Rich `Text` Diagnostics colour while rendering the owned app view.

## Deterministic acceptance

- [ ] `test_second_console_exits`
- [ ] `test_refresh_key_calls_collector_request_seam`
- [ ] `test_pr_tab_activation_requests_github_only`
- [ ] `test_diagnostics_rich_text_colour_is_preserved`
- [ ] `test_console_app_calls_no_send_seam`
- [ ] `test_console_never_sends_atm_message`
- [ ] `test_diagnostics_agent_icons_coloured`
- [ ] `test_r_forces_all_sources_pr_tab_forces_github`

No UI test uses sleeps, real time, threads, or delivery.
