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
| `cron(args: list[str]) -> dict` | function | Cron ticks. For atm-bd this runs `atm-bd cron <args>`. |
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
`hook_deadline_error(stage)`). Registration is an explicit operator act by the
lead (`atm hook register`), stored per team and phase, so the daemon never
executes a command it was not told to by a team member with that authority.
The CLI-side alternative (run in the caller's shell, before the send) was
rejected because cron ticks have no caller shell and the environment would
differ between the two paths.

Flow for a hooked task command:

1. CLI sends the command as today.
2. Router resolves the active hook for (team, phase, hook name). None: normal
   path.
3. Runner spawns the module with the request JSON, deadline `TIMEOUT_MS`.
4. Reply handling per 2.2. Actions are dispatched through the same internal
   command path the HTTP API uses, with `as` identity, hooks disabled for the
   dispatch.
5. One structured log entry per hook call (hook, team, phase, caller,
   duration, outcome, action count) and one per action (ADR-067).

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

**D-4 Engine.** Hand-rolled on the `cron` crate 0.17 (7-field, seconds
first, UTC), not tokio-cron-scheduler (comparison on #1873: stale maintenance,
open drift and DST bugs, 500 ms poll loop, current-thread hang). A
`CronWorker` in `DaemonWorkers` (`crates/atm-daemon-bootstrap/src/lib.rs:532-539`)
modeled on `AtmTempSweeperRuntime` (`atm_temp_sweeper_runtime.rs:59-166`):
`select!` on `sleep_until(next)` vs shutdown, `shutdown(deadline)` joins under
the replacement-drain deadline.

Rules:

- `next` is computed from `Utc::now()` after each run and after startup; a
  missed tick is never replayed (restart, sleep, or a long run skips to the
  next future occurrence).
- One run per schedule at a time: a tick that arrives while the previous run
  is still executing is skipped and logged.
- Invalid cron strings are refused at `atm cron start` (parse before write),
  so the daemon never loads one.
- A cron run's reply follows 2.2: actions are dispatched un-hooked as the
  schedule's `created_by` unless an action names `as`.

## 6. CLI

- `atm hook register <hook> --module <path> --phase <x> [--cwd <dir>]`
- `atm hook disable --phase <x> [<hook>]`
- `atm hook list [--phase <x>] [--json]`
- `atm cron start <name> --cron "<expr>" --module <path> --phase <x> [--args ...] [--cwd <dir>]`
- `atm cron end --phase <x> [<name>]`
- `atm cron list [--phase <x>] [--json]`
- `atm doctor`: a `hooks` section listing every active registration for the
  caller's team with the module's `doctor()` reply (or its error); never
  fatal to doctor.

Team comes from the caller context as for every atm command (ADR-066).

## 7. Sprint cut (by crate; hard dependencies name the consumed artefact)

| Sprint | Crates | Consumes |
| --- | --- | --- |
| bk-1 storage | `atm-storage`, `atm-storage-rusqlite` | - |
| bk-2 hook runner and task integration | `atm-daemon-bootstrap`, `atm-http-runtime` | bk-1's `HookRegistrationStore` trait and `HookRegistration` type |
| bk-3 cron worker | `atm-daemon-bootstrap` | bk-1's `CronScheduleStore`; bk-2's `HookRunner` (shared runner) |
| bk-4 CLI and doctor | `atm`, `atm-core` | bk-1 types for list output; bk-2 reply types for error text |
| bk-5 docs and ADR | `docs/` | all |

bk-2 and bk-4 can develop in parallel once bk-1's types are on the stack;
bk-3 waits for bk-2's runner. Each sprint carries its tests; hook execution
is tested with a fixture module and a paused clock (`start_paused = true`),
never a live daemon.

## 8. Out of scope

The python module and its roles (phase bj); hooks on non-task commands;
cross-host hook execution; any replay of missed cron ticks.
