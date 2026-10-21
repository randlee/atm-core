# Sprint bl-14 — console app collection controls

closure_type: boundary; target_boundary: ConsoleApp; branch:
sprint/bl-14-console-app; PR target: integrate/phase-bl; wave 2; external_ref:
#359; recommended_agent: trinity; recommended_model: gpt-6.1-sol. must_follow:
bl-1 (request contract); parallel_safe: all other wave-2 sprints.

Own app.py and test_app.py. Implement console-instance lock, persisted-state
consumption, Diagnostics Rich colour, and no-send app boundary. r calls bl-1
force request; PR activation requests GitHub only.

- [ ] test_second_console_exits
- [ ] test_refresh_key_forces_every_collector_past_due_gate
- [ ] test_refresh_lock_waits_for_existing_result_without_second_query
- [ ] test_pr_tab_activation_requests_github_only
- [ ] test_console_app_calls_no_send_seam

Quality/Jev presentation is bl-7; headers are bl-6; bl-12 proves equality.
