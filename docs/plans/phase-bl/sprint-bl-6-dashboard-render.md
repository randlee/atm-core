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
3. The `mmdflux` dependency remains pinned at `>=2.6.1`. The console builds
   Mermaid from persisted aggregate state: exactly one
   round node (`<id>("<label>")`) and one
   `style <id> stroke:#<hex>,color:#<hex>` line per sprint. It then renders
   with `mmdflux --format text --color always` and converts the resulting
   24-bit ANSI stream to Rich `Text`. It does not draw a layout or recolour
   mmdflux text by glyph: mmdflux draws each node outline in `stroke` and its
   text in `color`, including the rounded `╭ ╮ ╰ ╯` corners.
4. A node label contains the sprint ID and, in priority order, the active
   sprint agent's icon immediately left of the agent name (QA ahead of sanity
   ahead of dev); otherwise, when QA and every fix are green, the ANC-037/038
   CI icon and plain-text `#<pr>`; otherwise the sprint state icon (blue play
   for ready, blocked icon for blocked, not-started icon otherwise). The status
   table retains its ANC-036 one-icon-per-active-agent history; this one-agent
   rule is only for DAG nodes.
5. The style-line outline is green for a complete sprint with all fixes green,
   red for blocking failures even while a fix dev, sanity, or QA agent is
   working, and white for a first dev in progress or a not-started sprint. The
   no-mmdflux/failure fallback list uses the same icon-plus-ID cell form and
   state colours. A mmdflux failure retains the stale diagram and its age
   through the existing `DagPane` behaviour.
6. README names Nerd Font Mono as the alignment requirement.

## Deterministic acceptance and validation

Persisted fresh/stale/never-read fixtures cover header age, status-bar
wording, agents placeholder, DAG/fallback styles and TTY/plain show output.
DAG unit tests use captured deterministic mmdflux 2.6.1 fixtures: MMDS JSON
asserts node IDs, labels, and node-style strokes, while always-colour ANSI text
asserts conversion into Rich `Text`. They require neither an `mmdflux` binary
nor timing. A binary integration test is omitted unless existing repository
test conventions already support a skip-when-absent marker. If the rasterizer
proves too large for one sprint, the dev reports that to team-lead; do not
pre-split it. Run frozen ruff, pyright and pytest; this sprint does not close
TUI paths.

## Governing text (verbatim)

> "if you don't have valid data, don't force an error."

> "I spent a lot of time on it and the color doesn't show up hardly anywhere"

> "mmdflux has json format which is deterministic and from what I read better than raw mmd."

MMDS is specified by [mmdflux](https://github.com/kevinswiber/mmdflux) tag
`mmdflux-v2.6.1` in [docs/mmds.md](https://github.com/kevinswiber/mmdflux/blob/mmdflux-v2.6.1/docs/mmds.md)
and [docs/mmds.schema.json](https://github.com/kevinswiber/mmdflux/blob/mmdflux-v2.6.1/docs/mmds.schema.json):
`mmdflux --format mmds` (alias `json`) emits deterministic version 1 output
with `geometry_level` `layout` by default (or `routed`, which adds `edge.path`),
node `id`, `label`, centre `position` (`x`, `y`) and `size` (`width`,
`height`), edge `id`/`source`/`target`, and node styles under
`extensions["org.mmdflux.node-style.v1"].nodes.<id>` with `fill`, `stroke`,
and `color`.

### Acceptance checklist

- [ ] `test_header_shows_content_age_when_carried`: t0 content at t0+300 shows t0, 5m and typed reason.
- [ ] `test_header_no_age_when_fresh`: equal times have no age text.
- [ ] `test_status_bar_carried_since`: bar labels carried data, not failure time as data age.
- [ ] `test_agents_placeholder_first_failure`: failed members read has typed unavailable line.
- [ ] `test_show_ansi_on_tty_plain_otherwise`: TTY has ANSI; pipe does not.
- [ ] `test_dag_mermaid_round_node_and_style_per_sprint`: each persisted sprint emits one round Mermaid node and one style line.
- [ ] `test_dag_mmds_fixture_node_ids_labels_and_strokes`: captured MMDS JSON has the generated IDs, labels, and node-style strokes.
- [ ] `test_dag_ansi_fixture_converts_to_rich_text`: captured always-colour mmdflux ANSI becomes equivalent coloured Rich `Text`.
- [ ] `test_dag_label_qa_sanity_dev_priority`: the node label chooses QA, then sanity, then dev, with the chosen icon immediately left of the name.
- [ ] `test_dag_label_ci_pr_after_qa_and_fixes_green`: green QA and fixes show the ANC-037/038 CI icon and plain `#<pr>`.
- [ ] `test_dag_label_state_icons_ready_blocked`: ready and blocked node labels use their specified state icons.
- [ ] `test_dag_style_outline_green_red_white`: Mermaid style strokes cover complete, blocking, and initial/dev states.
- [ ] `test_dag_red_outline_while_fix_agent_active`: an active fix agent does not suppress the red blocking stroke.
- [ ] `test_dag_round_nodes_render_rounded_corners`: the captured ANSI output retains mmdflux's rounded-corner glyphs.
- [ ] `test_dag_fallback_list_same_cells_and_colours`: fallback cells preserve the DAG icon-plus-ID form and state colours.

## Non-closure

Quality/Diagnostics console paths remain bl-7; event delivery remains bl-3.
