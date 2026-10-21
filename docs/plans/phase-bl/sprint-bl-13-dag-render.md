# Sprint bl-13 — DAG renderer

closure_type: boundary; target_boundary: DagRender; branch: sprint/bl-13-dag-render; PR target: integrate/phase-bl; wave: 2; external_ref: #360; recommended_agent: trinity; recommended_model: gpt-6.1-sol; must_follow: bl-1 (`StateSnapshot` codec fixture); parallel_safe: bl-2, bl-3, bl-4, bl-5, bl-6, bl-7, bl-8, bl-10, bl-14.

Own only `dag.py` and `test_dag.py`. It renders persisted snapshots through the pinned mmdflux Mermaid/MMDS/ANSI contract; it owns no collection, header, app, or delivery code.

## Governing text (verbatim)

> "mmdflux has json format..."

> "the color doesn't show up hardly anywhere"

## Deliverables

1. Pin `mmdflux >= 2.6.1`; consume captured MMDS/ANSI fixtures (not a binary at test time), including rounded-corner glyphs.
2. Render labels with QA > sanity > dev priority and the status icon to the left of the sprint name. Render ANC-037/038 with CI icon and `#<pr>`.
3. Render ready, blocked, and not-started icons; use green/red/white strokes, including red while a fix agent is working. Keep fallback list cells and colours identical.

## Deterministic acceptance

- [ ] `test_dag_mermaid_round_node_and_style_per_sprint`
- [ ] `test_dag_mmds_fixture_node_ids_labels_and_strokes`
- [ ] `test_dag_ansi_fixture_converts_to_rich_text`
- [ ] `test_dag_label_priority_qa_sanity_dev_and_icon_left_of_name`
- [ ] `test_dag_anc_037_038_use_ci_icon_and_pr_number`
- [ ] `test_dag_state_icons_and_green_red_white_strokes`
- [ ] `test_dag_fix_agent_is_red_while_working`
- [ ] `test_dag_mmdflux_pin_and_rounded_corner_glyph_fixture`
- [ ] `test_dag_fallback_list_same_cells_and_colours`

No test launches mmdflux or relies on timing.
