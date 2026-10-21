# Sprint bl-7 — TUI collectors and persisted-state tabs

`BOUNDARY-ConsoleApp`; wave 2; consumes bl-1 collector records and bl-4
reviewer metadata. The TUI retains a console-instance lock and can request
individual collectors, but displays current persisted state whether cron or
TUI supplied it; it is not a global scheduler and never sends ATM messages.
PR activation requests GitHub; `r` requests collectors subject to their locks.

Keep the Quality two-pane reviewer/finding view, Jev API-failure do-not-enter
icon, typed/never-loaded placeholders, and Diagnostics colour. Deterministic
Textual tests inject collector/persisted records without sleeps or delivery.
