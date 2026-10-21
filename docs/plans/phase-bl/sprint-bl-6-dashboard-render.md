# Sprint bl-6 — dashboard header and show render

closure_type: boundary; target_boundary: DashboardRender; branch:
sprint/bl-6-dashboard-render; PR target: integrate/phase-bl; wave 2; external_ref:
#360; recommended_agent: trinity; recommended_model: gpt-6.1-sol. must_follow:
bl-1 (StateSnapshot); parallel_safe: every other wave-2 sprint.

Own render.py, status.py, show.py, README and test_render.py, test_status.py,
test_show.py. DAG rendering is bl-13; this sprint has no dag.py ownership.

> "if you don't have valid data, don't force an error."

> "I spent a lot of time on it and the color doesn't show up hardly anywhere"

1. Header/status render persisted content_at, carried age and oldest typed
reason; tick time cannot make carried content fresh.
2. First members failure renders agents unavailable: typed text; never-read is
labelled; show uses ANSI only on TTY.
3. README requires Nerd Font Mono alignment.

- [ ] test_header_shows_content_age_when_carried
- [ ] test_header_no_age_when_fresh
- [ ] test_status_bar_carried_since
- [ ] test_agents_placeholder_first_failure
- [ ] test_show_ansi_on_tty_plain_otherwise

No collection, state evaluation, DAG, or send closes here; composition is bl-12.
