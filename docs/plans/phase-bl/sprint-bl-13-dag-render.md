# Sprint bl-13 — DAG renderer

closure_type: boundary; target_boundary: DagRender; branch:
sprint/bl-13-dag-render; PR target: integrate/phase-bl; wave 2; external_ref:
#360; recommended_agent: trinity; recommended_model: gpt-6.1-sol. must_follow:
bl-1 (StateSnapshot); parallel_safe: all other wave-2 sprints.

Own only dag.py and test_dag.py. It renders persisted StateSnapshot using the
fixed mmdflux Mermaid/MMDS/ANSI contract; it owns no header/status/show,
collector, app, or delivery path.

- [ ] test_dag_mermaid_round_node_and_style_per_sprint
- [ ] test_dag_mmds_fixture_node_ids_labels_and_strokes
- [ ] test_dag_ansi_fixture_converts_to_rich_text
- [ ] test_dag_label_qa_sanity_dev_priority
- [ ] test_dag_style_outline_green_red_white
- [ ] test_dag_fallback_list_same_cells_and_colours

Fixtures are captured MMDS/ANSI; no mmdflux binary or timing is needed.
