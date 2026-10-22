# Sprint bl-6 — dashboard render
Implementation repository/worktree: `randlee/atm-bd-orchestration`; branch/PR target: `integrate/phase-bl`.

closure_type: boundary; target_boundary: DashboardRender; branch: sprint/bl-6-dashboard-render; PR target: integrate/phase-bl; wave: 2; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-7, bl-8, bl-10, bl-13.

## Scope boundary

Own `render.py`, `status.py`, `show.py`, their tests, and console README. Render persisted state only; do not collect, build a DAG, or deliver.

## Authoritative deliverables

1. Render header/status from persisted `content_at`, carried age, oldest typed error, and never-read state.
2. Tick time cannot make carried content fresh: fresh data has no carried age, carried data retains its carried-since status. Render members-source failure as `agents unavailable` with its typed reason.
3. `show.py` colours every icon it prints with the colour bl-7's `icons.py` assigns it when stdout is a TTY, and emits no ANSI escape codes otherwise. The DAG and fallback list are coloured by bl-13 and Diagnostics agent details by bl-12. Document Nerd Font Mono alignment.
4. An unknown member (unknown sprint, ci or pr state, or GitHub data off, rate-limited or truncated) renders as `?` in #2D81E0, never as the no-PR dash or ci none glyph; the pr cell otherwise renders the PR state icon then `[#n](url)`, and `-` only when the sprint has no PR.
5. Rendering the same snapshot twice yields byte-identical Markdown table output.

> "if you don't have valid data, don't force an error."

> "I spent a lot of time on it and the color doesn't show up hardly anywhere"

## Authoritative acceptance criteria

- [ ] `test_header_renders_persisted_content_at_and_carried_age`
- [ ] `test_members_failure_renders_agents_unavailable_with_typed_reason`
- [ ] `test_never_read_status_is_distinct_from_source_failure`
- [ ] `test_header_shows_content_age_when_carried`
- [ ] `test_header_no_age_when_fresh`
- [ ] `test_status_bar_carried_since`
- [ ] `test_show_ansi_on_tty_plain_otherwise` (asserts every icon, not only the unknown-member glyph, carries its anchored colour on a TTY)
- [ ] `test_readme_documents_nerd_font_mono_alignment`
- [ ] `test_unknown_member_renders_question_mark_in_2d81e0`
- [ ] `test_markdown_table_render_is_byte_identical_twice`

## Authoritative validation
```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q tests/test_render.py tests/test_status.py tests/test_show.py
```

## Non-closure

It renders persisted state only; bl-8 collects, bl-3 delivers, bl-13 renders the DAG, and bl-12 owns installed-console composition.
