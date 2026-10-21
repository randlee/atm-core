# Sprint bl-7 — TUI collectors and persisted-state tabs

`BOUNDARY-ConsoleApp`; wave 2; consumes bl-1 collector records and bl-4
reviewer metadata. The TUI retains a console-instance lock and can request
individual collectors, but displays current persisted state whether cron or
TUI supplied it; it is not a global scheduler and never sends ATM messages.
PR activation requests GitHub; `r` requests collectors subject to their locks.

Keep the Quality two-pane reviewer/finding view, Jev API-failure do-not-enter
icon, typed/never-loaded placeholders, and Diagnostics colour. Deterministic
Textual tests inject collector/persisted records without sleeps or delivery.

## Scope and owned paths

Own `app.py`, `icons.py`, `runs.py` and app/tab/icon/run tests. It consumes
bl-1 records and bl-4 reviewers, not a shared scheduler or delivery seam.

## Authoritative deliverables

1. Console-instance lock; `r` requests every collector, PR activation requests
   GitHub, and every tab renders persisted aggregate state without sending ATM.
2. Status header prints full typed error text and age; Quality has a labelled
   never-loaded row.
3. Quality has two panes: newest runs above; reviewer rows below with status,
   elapsed, API_FAILURE icon, reviewer filtering, contributing reviewers and
   filing reviewer marked; absent metadata says `reviewers: not recorded`.
4. `API_FAILURE` uses a verified Nerd Font do-not-enter codepoint distinct
   from a Jev negative verdict, with typed error detail.
5. Diagnostics preserves Rich `Text` colour.

## Deterministic acceptance and validation

Keep old test names for unchanged Quality split/filter/contributor, missing
reviewer, status, icon and Diagnostics behaviour. Add injected collector
requests/persisted-state tests. Run frozen ruff, pyright and pytest; no
sleeps, threads, or message delivery.
