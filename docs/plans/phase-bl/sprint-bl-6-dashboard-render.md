# Sprint bl-6 — dashboard render

closure_type: boundary; target_boundary: DashboardRender; branch: sprint/bl-6-dashboard-render; PR target: integrate/phase-bl; wave: 2; external_ref: none; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-7, bl-8, bl-10, bl-13, bl-14.

Own `render.py`, `status.py`, `show.py`, their tests, and console README. Render persisted state only; do not collect, build a DAG, or deliver.

## Deliverables and deterministic acceptance

1. Render header/status from persisted `content_at`, carried age, oldest typed error, and never-read state.
2. Render members-source failure as `agents unavailable` with its typed reason; document Nerd Font Mono alignment.

- [ ] `test_header_renders_persisted_content_at_and_carried_age`
- [ ] `test_members_failure_renders_agents_unavailable_with_typed_reason`
- [ ] `test_never_read_status_is_distinct_from_source_failure`
- [ ] `test_readme_documents_nerd_font_mono_alignment`
