# Sprint bl-6 — dashboard render from persisted state

`BOUNDARY-DashboardRender`; wave 2; consumes bl-1 aggregate state only.
Dashboard, status bar, and `show` render persisted state: carried content has
oldest-good age/reason and never-read data has a labelled placeholder. They do
not collect or schedule. Retain #360 DAG/fallback/TTY colour and Nerd Font
documentation plus first-failure agents placeholder; use deterministic
persisted-state fixtures.

## Relations, scope, and owned paths

Own `render.py`, `status.py`, `show.py`, README and render/status/show tests.
It consumes persisted aggregate state; it does not collect, evaluate, or send.

## Authoritative deliverables

1. Header and status bar print `content_at`, carried age, and oldest typed
   reason; tick time never makes carried content look fresh.
2. First members failure renders `agents unavailable: <typed text>`.
3. The console builds the DAG itself from `mmdflux --format json` MMDS output:
   nodes carry `id`, `label`, `position`, and `size`; edges carry `source` and
   `target`; `--geometry-level routed` additionally supplies edge waypoints.
   The captured fixture is from mmdflux 2.6.1 and is byte-identical across
   runs. The renderer draws that graph into a character grid; it does not
   recolour mmdflux text by glyph.
4. One named rounded-box style constant owns the rounded box and corner glyphs.
   A DAG cell contains, in priority order: its active sprint agent's icon
   immediately left of the agent name (sanity ahead of dev; QA ahead of
   sanity); otherwise, when QA and every fix are green, the ANC-037/038 CI
   icon and plain-text `#<pr>`; otherwise the sprint state icon (blue play for
   ready, blocked icon for blocked, not-started icon otherwise). The status
   table retains its ANC-036 one-icon-per-active-agent history; this
   one-agent rule is only for DAG cells.
5. A complete sprint with all fixes green has a green outline; blocking
   failures have a red outline even while a fix dev, sanity, or QA agent is
   working; a first dev in progress or a not-started sprint has a white
   outline. The no-mmdflux/failure fallback list uses the same icon-plus-ID
   cells and state colours. A mmdflux failure retains the stale diagram and
   its age through the existing `DagPane` behaviour.
6. README names Nerd Font Mono as the alignment requirement.

## Deterministic acceptance and validation

Persisted fresh/stale/never-read fixtures cover header age, status-bar
wording, agents placeholder, DAG/fallback styles and TTY/plain show output.
DAG tests inject a captured MMDS JSON fixture: they require neither an
`mmdflux` binary nor timing. If the rasterizer proves too large for one sprint,
the dev reports that to team-lead; do not pre-split it. Run frozen ruff,
pyright and pytest; this sprint does not close TUI paths.

## Governing text (verbatim)

> "if you don't have valid data, don't force an error."

> "I spent a lot of time on it and the color doesn't show up hardly anywhere"

### Acceptance checklist

- [ ] `test_header_shows_content_age_when_carried`: t0 content at t0+300 shows t0, 5m and typed reason.
- [ ] `test_header_no_age_when_fresh`: equal times have no age text.
- [ ] `test_status_bar_carried_since`: bar labels carried data, not failure time as data age.
- [ ] `test_agents_placeholder_first_failure`: failed members read has typed unavailable line.
- [ ] `test_dag_icons_coloured`: DAG icon spans carry anchored hex colour.
- [ ] `test_fallback_icons_coloured`: fallback icon spans carry anchored colour.
- [ ] `test_show_ansi_on_tty_plain_otherwise`: TTY has ANSI; pipe does not.
- [ ] `test_dag_built_from_mmds_json`: captured MMDS JSON creates the character-grid DAG without mmdflux.
- [ ] `test_dag_cell_active_agent_sanity_over_dev`: a sanity agent wins the DAG-cell active-agent priority.
- [ ] `test_dag_cell_qa_over_sanity`: a QA agent wins over sanity in that cell.
- [ ] `test_dag_cell_ci_pr_after_qa_green`: green QA and fixes show the ANC-037/038 CI icon and plain `#<pr>`.
- [ ] `test_dag_cell_state_icon_ready_blocked`: ready and blocked states use their specified state icons.
- [ ] `test_dag_outline_green_red_white`: completed, blocking, and initial/dev states select green, red, and white outlines.
- [ ] `test_dag_red_outline_while_fix_agent_active`: an active fix agent does not suppress the red blocking outline.
- [ ] `test_dag_rounded_glyphs`: the named rounded-box style constant supplies every rounded/corner glyph.
- [ ] `test_fallback_list_same_cells_and_colours`: fallback cells preserve the DAG icon-plus-ID form and state colours.

## Non-closure

Quality/Diagnostics console paths remain bl-7; event delivery remains bl-3.
