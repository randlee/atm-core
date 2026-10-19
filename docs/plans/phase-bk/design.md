# Design - phase-bk: atm task hooks and atm cron

Authority: Rand's rulings on atm-core #1873 (2026-10-18, two rounds, verbatim
there). This document turns them into the atm-core design. Items marked
**D-n** are decisions this document makes that the rulings do not state;
each is open to Rand's amendment before plan review.

## 1. Scope

atm-core only. Four capabilities:

1. **Task hooks**: every `atm task` subcommand (`assign`, `start`, `close`,
   `move`, `list`, `events`) may be hooked. All hooks are optional; a missing
   hook means the normal subcommand behavior.
2. **Hook registration** per team and phase: `atm hook register`,
   `atm hook disable`, `atm hook list`. The orchestration skill registers at
   phase start and disables at phase complete.
3. **`atm cron`**: `atm cron start`, `atm cron end`, `atm cron list`. Cron
   strings live in a new table in the atm database; the daemon runs each due
   schedule's hook with the same contract; schedules survive a daemon restart.
4. **`atm doctor`** calls the registered hook module's `doctor()` when run
   from the team the hook is registered for, and reports its output. File
   drift detection (published manifest of sc-sha per file) belongs to the
   plugin's own doctor, not to atm (Rand, 2026-10-18).

## 2. Hook contract

### 2.1 Request (one schema for every hooked command)

atm invokes the registered command with one JSON object on stdin:

```json
{
  "schema_version": "1.0.0",
  "hook": "task.close",
  "team": "atm-dev",
  "phase": "bj",
  "caller": { "identity": "trinity", "team": "atm-dev", "host": "mbp" },
  "request": { "...": "the subcommand's own fields, exactly as the HTTP API takes them" },
  "custom": { "...": "passed through untouched; atm defines nothing inside" }
}
```

`hook` is one of `task.assign`, `task.start`, `task.close`, `task.move`,
`task.list`, `task.events`, `cron.tick`. `request` is the same JSON the
daemon's HTTP API accepts for that command, so a hook can return it unchanged
as an action. `caller` is who ran the command (identity, team, host), from the
daemon's authenticated caller context, never from the hook's environment.

**D-1 Environment.** The hook runs with a fixed environment: `ATM_HOOK=1`,
`ATM_TEAM`, `ATM_PHASE`, `PATH`, `HOME`, and the registered `cwd`. Nothing
else is inherited. The JSON is the contract; the environment is redundant with
it, as the ruling anticipates.

### 2.2 Reply

Exactly one JSON object on stdout:

| Reply | Meaning |
| --- | --- |
| `{"success": false, "message": "<text>"}` | Refused. Nothing is written. The message is returned to the caller verbatim and logged. |
| `{"success": true}` | Pass-through. atm performs the normal subcommand logic. |
| `{"success": true, "atm-action": [ <cmd>, <cmd> ]}` | atm performs the listed commands in order instead of the normal logic. Each `<cmd>` is `{"command": "<name>", "as": "<identity>", "request": {...}}`, the same JSON the HTTP API takes. The list may contain the original command unchanged. |

Actions run **un-hooked**: no hook is consulted for a command that a hook
returned. `as` lets a repo-registered hook act as another identity (the lead);
every action is logged with the hook that requested it. A malformed reply, a
non-zero exit, or a timeout is a refusal with a typed message
(`HOOK.MALFORMED`, `HOOK.EXIT`, `HOOK.TIMEOUT`); the transition is not written.

### 2.3 Module contract (python)

The registered command is `python3 <module> <hook>`. The module declares:

| Symbol | Type | Use |
| --- | --- | --- |
| `TIMEOUT_MS` | `int` constant | atm's deadline for one invocation. The module reads it to finish inside it. Required. |
| `hook(request: dict) -> dict` | function | Task hooks. |
| `cron(args: list[str]) -> dict` | function | Cron ticks (`cron.tick`). For atm-bd this runs `atm-bd cron <args>`. |
| `stop(args: list[str]) -> dict` | function | Cron stop condition (`cron.stop`), called before each scheduled run of a schedule started with `--until-hook`; replies `{"stop": true, "reason": "<text>"}` or `{"stop": false}`. For atm-bd it returns stop once the phase bead is closed. |
| `doctor() -> dict` | function | Called by `atm doctor` from the registered team; the plugin's doctor runs its own published-manifest drift check inside it. |

atm reads `TIMEOUT_MS` from the file as text at registration
(`atm hook register` refuses a module without it); it never imports the
module in-process.

**D-2 No sha in atm.** Rand (2026-10-18): "when atm-bd is published, the sha
must be calculated and stored in a manifest for every file. then the installer
can check against the published manifest to verify the file has not changed.
we should do this for the entire skill. all agents/skills/python files." and
"the plug-in can run this check as part of 'doctor' it provides and IF a hook
is installed, 'atm doctor' would call hook.doctor() when called from the team
it is registered for." So atm stores no file sha and defines no header; the
published manifest and its check live in atm-bd-orchestration (issue filed
there). `atm doctor` resolves the active hooks for the caller's team and
calls each module's `doctor()` with the same runner and deadline as a hook,
printing the reply under a `hooks` section; a missing or failing `doctor()`
is reported, not fatal.

## 3. Where hooks run

**D-3 The daemon runs hooks.** One runner, the same for a CLI-triggered task
transition and for a cron tick, with the fixed environment of D-1 and the
caller taken from the authenticated request. Precedent: the daemon already
runs an operator-configured command for received messages
(`crates/atm-daemon-bootstrap/src/received_hook_selector.rs:465-498`:
`tokio::process::Command` with `kill_on_drop`, a deadline, explicit kill, typed
`hook_deadline_error(stage)`). Registration is an explicit operator act by a
roster member of the team (`atm hook register`; Rand 2026-10-18 on #1873:
"atm hook register would be done by phase-lead, which is not necessarily
team-lead. I would think any roster member can register the hooks." and
"so any roster member for both"), stored per team and phase, so the daemon
never executes a command it was not told to by a member of that team.
The CLI-side alternative (run in the caller's shell, before the send) was
rejected because cron ticks have no caller shell and the environment would
differ between the two paths.

Flow for a hooked task command:

1. CLI sends the command as today. The request already carries
   `caller_identity` and `caller_team`; the daemon adds its configured
   `local_host` as `caller.host` (`AtmConfig.local_host`, null when the
   operator has not set it).
   A hooked request is accepted only from `AuthenticatedIngress::Local`,
   the rule `task_move` already applies.
2. Router resolves the active hook for (team, hook name) from its in-memory
   snapshot (D-8). None: normal path, no store read.
3. Runner spawns the module with the request JSON, deadline `TIMEOUT_MS`.
4. Reply handling per 2.2. Actions are dispatched through the router's one
   existing dispatch entry (`dispatch_with_request_id`) with a private
   `hooks_disabled` flag, `as` identity, ingress `Local`; no second dispatch
   entry is added. The same loop is exposed to the cron crate as the sealed
   core trait `HookActionDispatcher` (D-4).
5. One structured log entry per hook call (hook, team, phase, caller,
   duration, outcome, action count) and one per action (ADR-067).

**D-8 Active hooks are an in-memory snapshot.** The router loads every
active registration once at construction (`active_all()`) and replaces the
snapshot after each committed `atm hook register` or `disable`. The task
request path never reads `hook_registration`, so a daemon with no
registrations takes exactly today's path (ANC-014) and the router's
capacity-1 sync bridge is not entered per request. Registration and cron
routes are accepted from `Local` ingress only, from any roster member of
the team (ADR-068 D5, Rand's round-4 ruling); no lead check.

## 4. Storage (ADR-061 governed interface, minor bump)

Two tables, both team-scoped:

- `hook_registration(team, phase, hook, module_path, cwd, timeout_ms,
  active, registered_by, registered_at, disabled_at)`; unique on
  (team, phase, hook).
- `cron_schedule(id, team, phase, name, cron_expr, module_path, cwd, args,
  timeout_ms, active, created_by, created_at, last_run_at, last_outcome,
  ended_at)`; unique on (team, phase, name).

`atm hook disable --phase <x>` sets `active = 0` on every row of the phase;
`atm cron end --phase <x>` does the same for schedules. `atm hook list` and
`atm cron list` read these rows (JSON with `--json`). The schema-reviewer
records the minor bump at plan review.

## 5. Scheduler

**D-4 Engine: its own crate behind one trait.** Rand (2026-10-18, PR
#1877): "I would expect atm cron to be in it's own crate w/ simple trait
exposing it's features." `atm cron` is the workspace crate `atm-cron`
(`crates/atm-cron`), the only implementation of the sealed core trait
`atm_core::boundary::CronScheduler` (`validate_expression`, `start`, `end`,
`list`, `run_now`). The crate depends on `atm-core`, `atm-storage`,
`tokio`, `chrono`, the `cron` crate 0.17 (7-field, seconds first, UTC; not
tokio-cron-scheduler, comparison on #1873: stale maintenance, open drift
and DST bugs, 500 ms poll loop, current-thread hang), `async-trait`,
`serde_json`, `tracing`; never on `atm-http-runtime`, `atm-daemon-bootstrap`,
`atm`, `atm-storage-rusqlite`. It runs hooks through the injected
`HookRunner` and dispatches actions through the injected sealed core trait
`HookActionDispatcher`, which the router implements. The daemon bootstrap
composes `AtmCron`, hands it to the router as `Arc<dyn CronScheduler>` (the
three cron routes call it, so parse-before-write and the reload signal live
in the crate, and `atm-http-runtime` carries no `cron` dependency and no
channel), and keeps its `CronWorkerHandle` in `DaemonWorkers`, modeled on
`AtmTempSweeperRuntime` (`atm_temp_sweeper_runtime.rs:59-166`): `select!` on
`sleep_until(next)` vs reload vs shutdown, `shutdown(deadline)` joins under
the replacement-drain deadline. A tick's actions run with ingress `Local`
as the schedule's `created_by` unless `as`.

Rules:

- `next` is computed from `Utc::now()` after each run and after startup; a
  missed tick is never replayed (restart, sleep, or a long run skips to the
  next future occurrence).
- One run per schedule at a time: a tick that arrives while the previous run
  is still executing is skipped and logged.
- Invalid cron strings are refused at `atm cron start` (parse before write,
  in `CronScheduler::start`), so the daemon never loads one.
- The schedule's `args` travel only inside the stdin request
  (`request = {"args": [...]}`); the module's argv is
  `python3 <module> cron.tick`, the same shape as every hook.
- A cron run's reply follows 2.2: actions are dispatched un-hooked as the
  schedule's `created_by` unless an action names `as`.

**D-9 Every schedule carries exactly one stop condition.** Rand (2026-10-18,
#1873 round 4): "there should probably be a 'stop condition' which was
mandetory when it is created which would generally be a time, but a
function/hook would also be useful. for atm-bd cron, phase bead closing
would be condition for stopping." `atm cron start` requires exactly one of
`--until <rfc3339>` or `--until-hook`, stored as
`cron_schedule.until_at` (nullable text) and `cron_schedule.stop_hook`
(integer 0/1) under a CHECK that exactly one is set; in code the pair is
`StopCondition { Until(DateTime<Utc>), Hook }` on `NewCronSchedule` and
`CronSchedule`. The stop check is the first step of every tick:

- `Until(t)`: the worker wakes at `min(next, t)`; when `now >= t` the
  schedule ends (`active = 0`, `ended_at = now`,
  `last_outcome = "stopped: until reached"`) and no run happens.
- `Hook`: before the run the daemon calls `python3 <module> cron.stop` with
  the same stdin JSON shape (hook `cron.stop`, `request = {"args": [...]}`)
  under the schedule's `timeout_ms`. `{"stop": true, "reason": "<text>"}`
  ends the schedule with `last_outcome = "stopped: <text>"` and no run;
  `{"stop": false}` runs the tick; a malformed reply, a non-zero exit or a
  timeout is `HOOK.MALFORMED` / `HOOK.EXIT` / `HOOK.TIMEOUT`, the tick is
  skipped and logged, the schedule stays active.

`atm cron end` remains the manual stop. A schedule that has ended is listed
until its phase's rows are cleaned up, like a disabled hook.

atm-bd usage: the phase lead starts the phase's cron with `--until-hook`
and the module's `stop()` returns `{"stop": true, "reason": "phase bead
closed"}` once the phase bead is closed; no `atm cron end` is needed at
phase close.

## 6. CLI

- `atm hook register <hook> --module <path> --phase <x> [--cwd <dir>]`
- `atm hook disable --phase <x> [<hook>]`
- `atm hook list [--phase <x>] [--json]`
- `atm cron start <name> --cron "<expr>" --module <path> --phase <x> (--until <rfc3339> | --until-hook) [--args ...] [--cwd <dir>]`
  (exactly one of `--until` and `--until-hook`, D-9)
- `atm cron end --phase <x> [<name>]`
- `atm cron list [--phase <x>] [--json]`
- `atm doctor`: a `hooks` section listing every active registration for the
  caller's team with the module's `doctor()` reply (or its error); never
  fatal to doctor.

Team comes from the caller context as for every atm command (ADR-066).
`atm hook register` and `atm cron start` are open to every roster member of
the team (Rand 2026-10-18 round 4: "so any roster member for both").

## 7. Sprint cut (by crate; hard dependencies name the consumed artefact)

| Sprint | Crates | Consumes |
| --- | --- | --- |
| bk-1 hook registration and cron schedule stores | `atm-storage` (two sealed store traits, row types, `StopCondition`, in-memory doubles), `atm-storage-rusqlite` (DDL, impls) | - |
| bk-2 core hook contract, sealed traits and protocol | `atm-core` (`hook/`, `boundary/{hook_runner,cron_scheduler,hook_action_dispatcher}.rs`, `boundary/mod.rs` re-exports, protocol variants), `atm-error`, `atm-runtime-test-support` (double re-exports) | bk-1's row types and store traits (the sealed traits take `HookRegistration`, `NewCronSchedule`, `CronSchedule`) |
| bk-3 hook runner and task integration | `atm-daemon-bootstrap` (runner), `atm-http-runtime` (consult, snapshot, six routes, `HookActionDispatcher` impl) | bk-2's `HookRequest`/`HookReply`/`StopReply`/`HookAction`, `hook_name_for`, the `HookRunner`, `CronScheduler`, `HookActionDispatcher` traits and the `ATM_HOOK_*` codes; bk-1's `HookRegistrationStore::active_all` |
| bk-4 atm-cron crate | `atm-cron` (new), workspace `Cargo.toml`, `boundaries/atm-cron` | bk-1's `CronScheduleStore`, `StopCondition`, `record_stop`; bk-2's `HookRequest::for_cron`, `StopReply`, the `HookRunner`, `CronScheduler`, `HookActionDispatcher` traits and `CronExprInvalid` |
| bk-5 CLI, doctor, docs and cron composition | `atm`, `atm-core` (`doctor/`), `atm-http-runtime` (router `doctor()` only), `atm-daemon-bootstrap` (composition only), `docs/user-documents` | bk-2's protocol variants and types; bk-3's `HookRunner::doctor` through the router, `with_cron_scheduler`; bk-4's `AtmCron`, `CronWorkerHandle` |

**D-7 Contract types live in bk-1, not bk-5.** The first cut placed every
`atm-core` change in bk-5. `atm-http-runtime -> atm-daemon-bootstrap` is a
forbidden edge (`boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`),
so the runner the router calls must be a core boundary trait implemented in
bootstrap, exactly as `MessageReceivedHookSelector` is
(`crates/atm-core/src/boundary/message_received_hook_emitter.rs:54`), and
bk-3 and bk-5 both consume the same request, reply and protocol types. Those
types are therefore the contract sprint's (bk-1); bk-5 keeps the CLI and the
doctor section of `atm-core`.

Waves: bk-1 (stores) alone; bk-2 (core contract) alone, since its sealed
traits take bk-1's row types; bk-3 (runner and router) and bk-4
(`atm-cron`) in parallel, since both consume only bk-2's traits and types
and own disjoint paths, rebased onto the stack in that order
(parallel-then-rebase, Rand 2026-10-12); bk-5 alone, since it composes
bk-4's crate into bk-3's router and bootstrap and edits bk-3's `doctor()`.
bk-5 is the integration sprint and carries the user documentation and the
cron composition; a docs-only sprint would be thin (atm-beads plan
guidelines). Each sprint carries its tests; hook execution
is tested with a fixture module and a paused clock (`start_paused = true`),
never a live daemon.

**D-5 One active registration per team and hook.** The task request carries
the team, not a phase, so the router resolves a hook by (team, hook). To keep
that resolution unambiguous, `atm hook register` refuses (`HOOK.ACTIVE`,
naming the phase) when another phase's registration for the same (team, hook)
is active; the lead disables that phase first. `atm cron start` has no such
rule: schedules are keyed by (team, phase, name) and every active schedule
runs.

**D-6 `request` and action encoding.** `request` is the serde JSON of the
`RequestEnvelope` variant the CLI sent (`crates/atm-core/src/protocol.rs:57`):
`task.assign` is a `Write` whose `task_op_normalized()` yields a task id and
no operation, `task.start` and `task.close` are `Write` with `task_op`
`Start` or `Close`, `task.move` is `TaskMove`, `task.list` and `task.events`
are `List` with `task_ledger` `Tasks` or `Events`. An action is
`{"command": "<hook name>", "as": "<identity>", "request": <that same
envelope JSON>}`; atm deserializes `request` with the envelope type, replaces
`caller_identity` with `as` when present, and dispatches it through the
router's own path with hooks disabled. An action is valid only when its
`command` is one of the six task hooks and its `request` is the envelope
of that same command (`hook_name_for(request) == command`); anything else
is `HOOK.MALFORMED`, so a hook can never dispatch a registry, cron or
other route as another identity.

## 8. Out of scope

The python module and its roles (phase bj); hooks on non-task commands;
cross-host hook execution; any replay of missed cron ticks.
