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
`args` (JSON array), `timeout_ms`, `jitter_s`, `custom` (JSON object,
ADR-068 D1), `active`, `created_by`, `created_at`,
`last_run_at`, `last_outcome`, `ended_at`, and the stop condition of D4 as
`until_at` (nullable text) and `stop_hook` (integer 0/1) under a CHECK that
exactly one is set. `atm cron end --phase <x> [<name>]` sets rows inactive
and keeps them; every active row, of every team, runs. The expression is
parsed before the row is written, so the daemon never loads an invalid
one (`ATM_CRON_EXPR_INVALID`). The grammar is the `cron` 0.17 one: 6 or 7
fields, seconds first, year optional, or one of `@yearly`, `@monthly`,
`@weekly`, `@daily`, `@hourly`, in UTC. A five-field expression, an
expression with no future occurrence and an `--until` at or before now are
refused. Restarting an ended name updates its row in place and keeps the
id. Any roster
member of the team may start a schedule; the route checks membership only
(round 4).

### D2. One crate, one sealed trait, one worker; UTC, no missed-tick replay, one run in flight

`atm cron` is its own workspace crate, `atm-cron`, the only implementation
of the sealed core trait `atm_core::boundary::CronScheduler`
(`start`, `end`, `list`); It reaches stores and hook types only through `atm_core::boundary`
(dependencies in `design.md`). It
runs hooks through the injected `HookRunner` and dispatches a reply's
actions through the injected sealed core trait `HookActionDispatcher`,
which `UnhookedRouter` implements below the task filter (ADR-068 D4), each
action with a fresh `SERVER_REQUEST_BUDGET`. Store calls run on
`spawn_blocking`. The daemon bootstrap composes the crate's `AtmCron`
once, installs it in the router (the three cron routes call the trait, so
parse-before-write and the reload signal are the crate's) and keeps the
worker handle in `DaemonWorkers`, started from the active rows at daemon
start and drained under the replacement-daemon deadline like the other
workers. Each schedule's next time is `Schedule::after(&(now -
jitter_s)).next() + jitter_s` for the schedule's fixed `jitter_s`, so a
restart inside an occurrence's jitter window still fires it, recomputed after startup, after each run and after a reload (ADR-065: UTC
under the hood); a tick missed through a restart, a sleep or a long run is
never replayed. A tick that arrives while the schedule's previous run is
still executing is skipped and logged. A run that panics is
caught and recorded, and the worker keeps ticking. Rand (2026-10-18):
"Random per-job offset so jobs don't all fire on :00 <-- we should do
this. it should be random over remaining slots." `start` draws `jitter_s`
once from an injected random source, uniform over the offsets in
`[0, min(period_s / 2, 1800))` not held by another active schedule with
the same expression (all of them when every offset is held); it is
stored, so a restart keeps it. A restart in
place sets `created_at` to now, and a reload keeps a worker slot only when
the reloaded row is unchanged (id, `created_at`, expression, jitter, stop,
module, cwd, args, timeout, custom); otherwise the slot's in-flight run is
aborted and its outcome never recorded. This reading of "remaining slots" is the design's, open to
Rand's correction. A `start` or `end` through the trait
signals the worker to reload its rows, so no restart is needed.

### D3. A tick is a hook run

The tick sends ADR-068's request with `hook = "cron.tick"` and
`request = {"args": [...], "created_by": "<agent>"}` and the schedule's
`custom` to the module (`python3 <module> cron.tick`; the args travel only
in the stdin request, never on argv), so one module serves many schedules
that differ in `custom` and `created_by`. It runs under the schedule's
`timeout_ms`, with the fixed environment. Rand (2026-10-18): "the cron
timeout would need to be much longer than the hook timeout. 2.5s is fine
for hook. cron might query gh, query bd, query atm aggregate
information.... something more like a 30s timeout would be more
reasonable, although I expect they will generally be much faster". That
value is the module's `CRON_TIMEOUT_MS`, else `TIMEOUT_MS`, at most
`CRON_TIMEOUT_MAX_MS` = 30 000, checked by the cron start route, and each dispatched action gets its own
`SERVER_REQUEST_BUDGET`. The reply follows
ADR-068 D2: refused is recorded as `refused: <message>` with the schedule kept
active, pass-through and not-applicable are recorded as ok, and a handled
reply's actions are dispatched un-hooked as the schedule's `created_by` unless an action
names `as`, with ingress `Local`. Each run writes `last_run_at` and
`last_outcome` on the row and one structured entry (ADR-067) with
`subsystem = "atm_cron"`.

### D4. Every schedule has exactly one stop condition, checked first on every tick

`atm cron start` requires exactly one of `--until <rfc3339>` and
`--until-hook` (`StopCondition { Until { at: DateTime<Utc> }, Hook }` on
`CronStartSpec` and the stored schedule). The stop check precedes the run on every tick:

- `Until { at: t }`: the worker wakes at `min(next, t)`; once `now >= t` the
  schedule ends (`active = 0`, `ended_at`, `last_outcome = "stopped: until
  reached"`) and nothing runs.
- `Hook`: the daemon calls `python3 <module> cron.stop` with the same stdin
  shape (`hook = "cron.stop"`, the D3 `request` and `custom`) under the
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
capabilities is amended to fourteen: `HookRegistrationStore` (ADR-068 D1,
D5) and `CronScheduleStore` (D1, D4).

## Consequences

- `cron_schedule` is an additive SQLite MINOR object (ADR-061 D6 record of
  2026-10-18, shared with `hook_registration`).
- The `cron` crate enters the workspace for `atm-cron` only;
  `tokio-cron-scheduler` is not adopted; `atm-http-runtime` gains no cron
  dependency and no channel.
- `boundaries/atm-cron/cron-scheduler.toml`,
  `boundaries/atm-core/cron-scheduler.toml` and
  `boundaries/atm-core/hook-action-dispatcher.toml` pin the edges:
  `atm-cron` may be depended on by `atm-daemon-bootstrap` only.
- `cron.stop` is the eighth hook name (six task hooks, `cron.tick`,
  `cron.stop`); `atm hook register` accepts neither cron name.

## BK-9 amendment — transition intents and sender authority

Cron transitions are persisted through a generation-fenced, sealed transition
store. State and every lifecycle/module-mail intent commit atomically. A stale
generation changes neither. A logical intent is exactly once per transition;
unknown external handoffs retain a bounded typed retry cause and appear only
as reconciliation-required pending records, never blind-retry work. They
remain reconcilable rather than being claimed as externally exactly once.
Lifecycle notices target the persisted creator and are rendered by the
dedicated `cron` template family.

Cron action sender precedence is explicit ordinary `from`, legacy same-team
`as`, then daemon-owned `cron@tool`. Mixed `from` and `as`, foreign/different
senders, and explicit virtual senders are refused. A scheduler-minted permit
fences each action admission after manual end/restart. No capability is passed
to a module or represented by child environment values.
