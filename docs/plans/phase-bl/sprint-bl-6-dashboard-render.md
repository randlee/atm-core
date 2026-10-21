# Sprint bl-6 — dashboard render from persisted state

`BOUNDARY-DashboardRender`; wave 2; consumes bl-1 aggregate state only.
Dashboard, status bar, and `show` render persisted state: carried content has
oldest-good age/reason and never-read data has a labelled placeholder. They do
not collect or schedule. Retain #360 DAG/fallback/TTY colour and Nerd Font
documentation plus first-failure agents placeholder; use deterministic
persisted-state fixtures.
