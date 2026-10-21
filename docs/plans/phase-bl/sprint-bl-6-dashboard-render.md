# Sprint bl-6 — dashboard render: data age over carried content, agents placeholder, and colour

closure_type `boundary`; target_boundary `BOUNDARY-DashboardRender`
(`render.dashboard`, `render.status_bar`, `status.py`, `atm-bd show`). Branch
`sprint/bl-6-dashboard-render`, PR → `sprint/bl-5-sanity-flow` (the refresh contract producer). Wave 2.
Priority: the clock rows are P1 (TUI showing improper information); #360 is
P3 (a diagnosis aid).
`external_ref`: `gh-126,gh-360`.
recommended_agent `arch-ctm`; recommended_model `sonnet`.

**Hard dependencies.**
- must_follow bl-1: consumes `Snapshot.content_at`, `PollHealth` failures
  restored from `health.json`.

Paths under `apps/atm-bd-console/src/atm_bd_console/`; line numbers at
v0.18.4 (86853d5).

## Governing text (verbatim)

Rand, 2026-10-20: "if you don't have valid data, don't force an error."


Rand on #360: "I spent a lot of time on it and the color doesn't show up
hardly anywhere"

The 2026-10-21 refresh anchor and the relayed correction are in `design.md` §1.

## Relations

`must_follow` bl-1: consumes `Snapshot.content_at` and persisted health. `parallel_safe`: bl-2, bl-3, bl-4, bl-5,
bl-7, bl-8; its owned render/status/show paths are disjoint from each. bl-9
consumes this sprint's rendered age surface.

## Scope boundary

In: console-audit rows still open at v0.18.4 that live in the dashboard:
row 1 (header clock shows the tick time over carried data), row 12 (status
bar labels a failure time `failed HH:MM` and shows no carried age,
`render.py:268-313`), row 3 (agents placeholder missing on a first-ever
failure with no `PollHealth` entry, `render.py:177-179`); #360 for the DAG,
fallback list and `atm-bd show` paths. #359 label data is owned by bl-3. Out: the Diagnostics path
of #360 (`app.py`, bl-7), every `app.py` tab (bl-7).

## Owned paths

- `render.py`, `status.py`, new `show.py`
- `apps/atm-bd-console/README.md` (Nerd Font Mono note)
- tests: `tests/test_render.py`, `tests/test_status.py`, new `tests/test_show.py`

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
