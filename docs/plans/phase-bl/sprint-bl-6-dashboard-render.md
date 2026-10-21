# Sprint bl-6 — dashboard render

closure_type: boundary; target_boundary: DashboardRender; branch: sprint/bl-6-dashboard-render; PR target: integrate/phase-bl; wave: 2; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-7, bl-8, bl-10, bl-13, bl-14.

Own `render.py`, `status.py`, `show.py`, their tests, and console README. Render persisted state only; do not collect, build a DAG, or deliver.

## Deliverables and deterministic acceptance

1. Render header/status from persisted `content_at`, carried age, oldest typed error, and never-read state.
2. Tick time cannot make carried content fresh: fresh data has no carried age, carried data retains its carried-since status. Render members-source failure as `agents unavailable` with its typed reason.
3. `show.py` emits ANSI only on a TTY and plain text otherwise; document Nerd Font Mono alignment.

> "if you don't have valid data, don't force an error."

> "I spent a lot of time on it and the color doesn't show up hardly anywhere"

- [ ] `test_header_renders_persisted_content_at_and_carried_age`
- [ ] `test_members_failure_renders_agents_unavailable_with_typed_reason`
- [ ] `test_never_read_status_is_distinct_from_source_failure`
- [ ] `test_header_shows_content_age_when_carried`
- [ ] `test_header_no_age_when_fresh`
- [ ] `test_status_bar_carried_since`
- [ ] `test_show_ansi_on_tty_plain_otherwise`
- [ ] `test_readme_documents_nerd_font_mono_alignment`

## Validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q tests/test_render.py tests/test_status.py tests/test_show.py
```
