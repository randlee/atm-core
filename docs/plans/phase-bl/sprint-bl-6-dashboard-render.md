# Sprint bl-6 — dashboard render: data age over carried content, agents placeholder, DAG labels and colour

closure_type `boundary`; target_boundary `BOUNDARY-DashboardRender`
(`render.dashboard`, `render.status_bar`, the Mermaid label in
`model._sprint_views`, `status.py`, `atm-bd show`). Branch
`sprint/bl-6-dashboard-render`, PR → `sprint/bl-5-sanity-flow` (layer 6;
cut from the bl-3 head after bl-1, rebased in order). Wave 2.
Priority: the clock rows are P1 (TUI showing improper information); #359 and
#360 are P3 (diagnosis aids).
`external_ref`: `gh-126,gh-359,gh-360`.
recommended_agent `arch-ctm`; recommended_model `sonnet`.

**Hard dependencies.**
- must_follow bl-1: consumes `Snapshot.content_at`, `PollHealth` failures
  restored from `health.json`, and bl-1's edits to `cli.py` (this sprint
  edits `atm-bd show` in the same file).
- must_follow bl-3: `model.py` (bl-3 edits `_problems`/`_pr_rows`; this
  sprint edits the label region `:314-383`).

Paths under `apps/atm-bd-console/src/atm_bd_console/`; line numbers at
v0.18.4 (86853d5).

## Governing text (verbatim)

Rand, 2026-10-20: "if you don't have valid data, don't force an error."

Rand on #359: "the boxes have a lot of crap I dont' want anyway" and "that
would be better, although what you showed, has a space missing and is not
even".

Rand on #360: "I spent a lot of time on it and the color doesn't show up
hardly anywhere"

The 2026-10-21 refresh anchor and the relayed correction are in `design.md` §1.

## Scope boundary

In: console-audit rows still open at v0.18.4 that live in the dashboard:
row 1 (header clock shows the tick time over carried data), row 12 (status
bar labels a failure time `failed HH:MM` and shows no carried age,
`render.py:268-313`), row 3 (agents placeholder missing on a first-ever
failure with no `PollHealth` entry, `render.py:177-179`); #359 entire; #360
for the DAG, fallback list and `atm-bd show` paths. Out: the Diagnostics path
of #360 (`app.py`, bl-7), every `app.py` tab (bl-7).

## Owned paths

- `render.py`, `model.py` (label region `:314-383` only), `status.py`,
  `cli.py` (`show`, `:98-101`)
- `apps/atm-bd-console/README.md` (Nerd Font Mono note)
- tests: `tests/test_render.py`, `tests/test_model.py` (after bl-3),
  `tests/test_status.py`, `tests/test_cli.py` (after bl-1)

`icons.py` is not edited here (bl-7 owns it); the glyph → style map for
recolouring is built in `status.py` from the `icons.py` tables.

## Authoritative deliverables

1. **Header and status bar never look fresh over carried data (rows 1, 12).**
   `render.dashboard` prints `content_at` and its age next to the clock
   whenever `content_at < generated_at`, with the oldest failing source's
   typed text (`error_text`). The status bar shows `carried since <t> (<age>)`
   and labels a failure time as such ("last failed HH:MM"), never as data age.
2. **Agents placeholder (row 3).** When the members read failed and no
   agents were ever read, the agents section prints
   `agents unavailable: <typed text>` whether or not a `PollHealth` failure
   exists.
3. **#359 labels.** `_sprint_views` builds each node label as exactly
   `<state icon> <sprint id>` plus ` <agent icon>` per active agent, single
   spaces, no role word, no agent name, no title lines (drop
   `_label_title_lines` from the label; the title stays in the sprint list).
   Pad every label to the widest so mmdflux boxes have equal width.
4. **#360 colour (DAG, fallback, show).** Recolour mmdflux output after
   layout by mapping each known glyph to its anchored style; keep Rich `Text`
   in the fallback list (`status.py:42,45`) instead of `.plain`; `atm-bd show`
   emits ANSI colour when stdout is a TTY and plain text otherwise.
5. **README.** Note that a terminal font without Nerd Font glyphs breaks box
   alignment and the fix is a "Nerd Font Mono" font.

## Authoritative acceptance criteria (deterministic)

gh-126 (refresh: clocks)
- [ ] `test_header_shows_content_age_when_carried`: snapshot with `content_at` = t0, `generated_at` = t0+300 → header text contains t0's clock and "5m" and the typed reason.
- [ ] `test_header_no_age_when_fresh`: equal times → no age text.
- [ ] `test_status_bar_carried_since`: the bar shows `carried since` and never `failed HH:MM` as a data time.
- [ ] `test_agents_placeholder_first_failure`: members Err, no prior agents, no health entry → `agents unavailable:` line.

gh-359
- [ ] `test_node_label_exact`: fixed snapshot → label `"<icon> bl-1 <agent icon>"` with single spaces and no title text.
- [ ] `test_mmdflux_boxes_equal_width`: render a fixed 4-sprint snapshot through mmdflux; every box top-border line has the same length and every `│` line inside a box ends at the same column.
- [ ] `test_readme_nerd_font_note`: README contains "Nerd Font Mono".

gh-360
- [ ] `test_dag_icons_coloured`: each icon span in the recoloured DAG `Text` carries its anchored hex colour.
- [ ] `test_fallback_icons_coloured`.
- [ ] `test_show_ansi_on_tty_plain_otherwise`: with a fake TTY stdout the output contains the ANSI colour sequence for an anchored colour; with a pipe, none.

## Authoritative validation

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

## Non-closure

#360 closes when this sprint and bl-7 (Diagnostics path) are merged.
