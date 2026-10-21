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
3. DAG/fallback glyphs retain anchored colour; `show` uses ANSI only on TTY.
4. README names Nerd Font Mono as the alignment requirement.

## Deterministic acceptance and validation

Persisted fresh/stale/never-read fixtures cover header age, status-bar
wording, agents placeholder, DAG/fallback styles and TTY/plain show output.
Run frozen ruff, pyright and pytest; this sprint does not close TUI paths.
