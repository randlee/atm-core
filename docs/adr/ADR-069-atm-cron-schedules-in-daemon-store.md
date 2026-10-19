# ADR-069 — `atm cron`: Schedules Persist In The Daemon Store, One Crate Behind One Trait Ticks Them

| Field | Value |
| --- | --- |
| ID | ADR-069 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | `atm cron start|end|list`, the `cron_schedule` table, the `atm-cron` crate and the `atm_core::boundary::CronScheduler` trait, the `cron.tick` and `cron.stop` hook requests, the mandatory stop condition |
| Relates to | atm-core issue #1873; `docs/plans/phase-bk/design.md` (§4, §5, D-4); ADR-068 (hook contract the tick reuses); ADR-061 (storage MINOR); ADR-063 D3 and ADR-036 (capability inventory, amended by D5); ADR-065 (UTC under the hood); ADR-067 (one structured log) |

## Context

Rand's rulings on #1873 (2026-10-18, verbatim):

- "Note: I have a cron design I want to add i.e. 'atm cron' which is not a
  hard deliverable, however it wouldn't be a bad idea to consider it now.
  the cron design would be 'atm cron' and it would run a hook (just like
  the atm task hooks) at a predefined interval AND return a set of atm
  commands just like we described above."
- The cron function lives in the same hook module and is registered at
  phase start and shut down at phase complete; for atm-bd the function runs
  `atm-bd cron <args>`.
- "`atm cron`": a simple scheduler; cron strings stored in a new atm db
  table; subcommands `atm cron start`, `atm cron end`, `atm cron list`;
  schedules survive a daemon restart.
- On PR #1877 (2026-10-18): "I would expect atm cron to be in it's own
  crate w/ simple trait exposing it's features."
- Round 4 (#1873 comment 6257077671): "atm cron start basically allows
  anything to get registeted, from a general sense any agent could set up
  a cron. for atm-bd, phase-lead would set up cron job gated by the phase.
  there should probably be a 'stop condition' which was mandetory when it
  is created which would generally be a time, but a function/hook would
  also be useful. for atm-bd cron, phase bead closing would be condition
  for stopping." and "so any roster member for both".

The scheduler-crate comparison recorded on #1873 chose the `cron` crate
(0.17) with a hand-rolled loop in the existing daemon-worker pattern over
`tokio-cron-scheduler`: fewer dependencies, two 2026 releases, and the
overlap, deadline and shutdown rules are the daemon's anyway.

## Decision

### D1. Schedules are rows in the daemon store

`cron_schedule` is an additive SQLite table keyed by `id` with
`UNIQUE (team, phase, name)`, holding the cron expression, module path, cwd,
`args` (JSON array), `timeout_ms`, `active`, `created_by`, `created_at`,
`last_run_at`, `last_outcome`, `ended_at`, and the stop condition of D4 as
`until_at` (nullable text) and `stop_hook` (integer 0/1) under a CHECK that
exactly one is set. `atm cron end --phase <x> [<name>]` sets rows inactive
and keeps them; every active row, of every team, runs. The expression is
parsed with `cron::Schedule::from_str` before the row is written, so the
daemon never loads an invalid one (`ATM_CRON_EXPR_INVALID`). Any roster
member of the team may start a schedule; the route checks membership only
(round 4).

### D2. One crate, one sealed trait, one worker; UTC, no missed-tick replay, one run in flight

`atm cron` is its own workspace crate, `atm-cron`, the only implementation
of the sealed core trait `atm_core::boundary::CronScheduler`
(`validate_expression`, `start`, `end`, `list`, `run_now`). The crate
depends on `atm-core`, `atm-storage`, `tokio`, `chrono`, `cron` and
nothing of `atm-http-runtime`, `atm-daemon-bootstrap` or the CLI; it runs
hooks through the injected `HookRunner` and dispatches a reply's actions
through the injected sealed core trait `HookActionDispatcher`, which the
HTTP router implements. The daemon bootstrap composes the crate's `AtmCron`
once, hands it to the router (the three cron routes call the trait, so
parse-before-write and the reload signal are the crate's) and keeps the
worker handle in `DaemonWorkers`, started from the active rows at daemon
start and drained under the replacement-daemon deadline like the other
workers. Each schedule's next time is `Schedule::after(&now).next()`,
recomputed after startup, after each run and after a reload (ADR-065: UTC
under the hood); a tick missed through a restart, a sleep or a long run is
never replayed. A tick that arrives while the schedule's previous run is
still executing is skipped and logged. A `start` or `end` through the trait
signals the worker to reload its rows, so no restart is needed.

### D3. A tick is a hook run

The tick sends ADR-068's request with `hook = "cron.tick"` and
`request = {"args": [...]}` to the module (`python3 <module> cron.tick`;
the args travel only in the stdin request, never on argv), under the
schedule's `timeout_ms`, with the fixed environment. The reply follows
ADR-068 D2: refused is recorded, pass-through is recorded as ok, actions
are dispatched un-hooked as the schedule's `created_by` unless an action
names `as`, with ingress `Local`. Each run writes `last_run_at` and
`last_outcome` on the row and one structured entry (ADR-067) with
`subsystem = "atm_cron"`.

### D4. Every schedule has exactly one stop condition, checked first on every tick

`atm cron start` requires exactly one of `--until <rfc3339>` and
`--until-hook` (`StopCondition { Until(DateTime<Utc>), Hook }` on
`NewCronSchedule`). The stop check precedes the run on every tick:

- `Until(t)`: the worker wakes at `min(next, t)`; once `now >= t` the
  schedule ends (`active = 0`, `ended_at`, `last_outcome = "stopped: until
  reached"`) and nothing runs.
- `Hook`: the daemon calls `python3 <module> cron.stop` with the same stdin
  shape (`hook = "cron.stop"`, `request = {"args": [...]}`) under the
  schedule's `timeout_ms`; the module's `stop(args)` replies
  `{"stop": true, "reason": "<text>"}` (the schedule ends with
  `last_outcome = "stopped: <text>"`, nothing runs) or `{"stop": false}`
  (the tick runs). A malformed reply, non-zero exit or timeout is the
  matching `ATM_HOOK_*` error: the tick is skipped and logged and the
  schedule stays active.

`atm cron end` remains the manual stop. For atm-bd the phase lead starts
the phase cron with `--until-hook` and `stop()` returns stop once the phase
bead is closed.

### D5. Amendment to ADR-063 D3: fourteen semantic storage capabilities

Rand, round 5 (#1873 comment 6257361348, 2026-10-18, verbatim): "yes, we
need storage for cron". ADR-063 D3's inventory of twelve semantic storage
capabilities ("No thirteenth semantic capability is authorized by this
ADR") is amended here, in this one place, to fourteen:

13. `HookRegistrationStore` (ADR-068 D1, D5): one active registration per
    team and hook, `disable` keeps the row.
14. `CronScheduleStore` (D1, D4): schedules with a mandatory stop
    condition, `end` and `record_stop` keep the row.

Both are sealed with `atm_storage::contract::sealed::Sealed`, implemented
only by `atm-storage-rusqlite`, re-exported through `atm_core::boundary`,
with in-memory doubles in `atm-runtime-test-support` only. Sprint bk-1
updates ADR-036's inventory and `docs/atm-storage/boundaries.md` and owns
those paths. No fifteenth capability is authorized by this ADR; the
ADR-063 review check now reads "fourteen".

## Consequences

- `cron_schedule` is an additive SQLite MINOR object (ADR-061 D6 record of
  2026-10-18, shared with `hook_registration`).
- The capability inventory is fourteen (D5); a storage trait outside that
  list is a finding.
- The `cron` crate enters the workspace for `atm-cron` only;
  `tokio-cron-scheduler` is not adopted; `atm-http-runtime` gains no cron
  dependency and no channel.
- `boundaries/atm-cron/cron-scheduler.toml`,
  `boundaries/atm-core/cron-scheduler.toml` and
  `boundaries/atm-core/hook-action-dispatcher.toml` pin the edges:
  `atm-cron` may be depended on by `atm-daemon-bootstrap` only.
- `cron.stop` is the eighth hook name (six task hooks, `cron.tick`,
  `cron.stop`); `atm hook register` accepts neither cron name.
- Review check: a replayed missed tick, two concurrent runs of one
  schedule, a schedule loaded from anything but the store, a cron tick
  that bypasses the hook runner or the stop check, a schedule row without
  a stop condition, or a cron dependency outside `atm-cron`, is a finding.
