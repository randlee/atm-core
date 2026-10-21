# Design - phase-bk: atm task hooks and atm cron

Authority: Rand's rulings on atm-core #1873 (2026-10-18, two rounds, verbatim
there). This document turns them into the atm-core design. Items marked
**D-n** are decisions this document makes that the rulings do not state;
each is open to Rand's amendment before plan review.

## 1. Scope

atm-core only. Four capabilities:

1. **Task hooks**: every `atm task` subcommand Rand listed (`assign`,
   `start`, `close`, `move`, `list`, `events`) may be hooked; `atm task
   history` is not (D-h). All hooks are optional; a missing hook means the
   normal subcommand behavior.
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
  "request": { "...": "the bare HTTP body of the hooked command's route" },
  "custom": { "goal": "phase bj sprint bj-3 is done" }
}
```

`hook` is one of the eight names `task.assign`, `task.start`, `task.close`,
`task.move`, `task.list`, `task.events`, `cron.tick`, `cron.stop`.
`request` is the bare HTTP request body the CLI sent for that command's
route, exactly as `encode_request_body` emits it
(`crates/atm-core/src/api.rs:364`), never the tagged `RequestEnvelope`
JSON; for `cron.tick` and `cron.stop` it is `{"args": [...],
"created_by": "<agent>"}`, where `created_by` is the agent that started
the schedule. The filter
takes it from `TaskCommand::request_body()` (§9.3). `caller` is who ran
the command (identity, team, host), from the request's own
`caller_identity` and `caller_team` and the daemon's configured
`local_host`, never from the hook's environment.

**D-l Custom JSON at registration.** Rand (2026-10-18), superseding the
review's empty-object reading: "I think we will want custom json because
we don't need to write a python module for every agent/team/repo that
want the same thing. for instance if we created /jev-goal which sent a
prompt to llm to determine if we were done, I would expect this would be
a single python module that was passed some custom-context info and
agent, repo, team. then an llm would collect context, pass to jev and
return the answer." `atm hook register` and `atm cron start` take
`--custom <json>`, a JSON object stored with the registration or the
schedule (`custom TEXT NOT NULL DEFAULT '{}'`). Every request built from
that row carries it in `custom` unchanged, byte-identical to
`serde_json::to_string` of the stored map. atm never reads a key inside
it. The route refuses a non-object or a serialized size above 65 536
bytes (`HOOK_CUSTOM_MAX_BYTES`) with `ATM_HOOK_MODULE_INVALID`; absent
means `{}`.

The motivating case is one `jev_goal.py` module and many schedules. Each
schedule names the same module with its own `custom` (the goal text, the
context to collect) and its own `created_by`. The module reads `team`
from the request, the agent from `request.created_by` (cron) or `caller`
(task hooks), the repo from its `cwd`, asks an LLM to collect context,
passes it to Jev and returns the answer as actions. No per-agent or
per-repo module is written.

**D-1 Environment.** The hook runs with a fixed environment: `ATM_TEAM`,
`ATM_PHASE`, `PATH`, `HOME`, and the registered `cwd`. Nothing
else is inherited. `PATH` and `HOME` are copied from the daemon's
environment once, when the runner is constructed. On unix `PATH` and
`HOME` are required. On Windows `HOME` is optional, `SystemRoot` and
`USERPROFILE` are required and copied, `PATH` is looked up
case-insensitively (`Path` matches), and the interpreter `python` is
resolved on it with `PATHEXT` (D-n). A missing required variable makes
every call `ATM_HOOK_EXIT` naming it. The JSON is the contract; the environment is
redundant with it, as the ruling anticipates.

### 2.2 Reply

Exactly one JSON object on stdout, parsed by the rules of §9.4:

| Reply | Meaning |
| --- | --- |
| `{"result": "handled", "atm-action": [ <cmd>, ... ]}` | Handled. atm performs the listed commands in order instead of the normal logic, and the chain stops. Each `<cmd>` is `{"command": "<atm command>", "as": "<identity>", "request": {...}}`, `request` being the bare HTTP body of that command's route (§9.2). |
| `{"result": "pass-through"}` | Pass-through. The hook owns this task; atm performs the normal subcommand logic, and the chain stops. |
| `{"result": "not-applicable"}` | Not this hook's task. The next hook in the chain runs (D-5); when every hook is not-applicable, atm performs the normal subcommand logic. |
| `{"result": "refused", "message": "<text>"}` | Refused. Nothing is written. The message is returned to the caller verbatim as `ATM_HOOK_REFUSED` and logged, and the chain stops. |

**Four results.** Rand (2026-10-18, verbatim): "we should add a 3-rd return type. instead of success=true/false which signals pass-through, we could return enum with { handled => run my actions, not-applicable, pass-through }". The refused
result is kept from Rand's earlier ruling ("bead {bead-id} is not ready").
The key sets are exact; anything else, including the earlier `success`
key, is `ATM_HOOK_MALFORMED` to the caller (§9.4).
On a cron tick, `handled` runs the actions; `pass-through` and
`not-applicable` both mean nothing to do (`last_outcome` `ok`); `refused`
records `last_outcome` `refused: <message>` and the schedule stays active.

**Actions (D-a).** Rand: "array of 'un-hooked' atm commands ... exactly the
same as they would be passed over http", with `atm send
omega-prime@hermes <escalation>` as the example. An action may be any atm
command that has an HTTP route, except `hook register|disable|list` and
`cron start|end|list`: a reply may not register hooks or schedules.
`command` is the atm command name (`send`, `ack`, `task.assign`, ...);
§9.2 maps each name to its request type. An unknown or refused command is
`ATM_HOOK_MALFORMED`. Actions always run un-hooked: they enter the normal
input stream below the hook filter (§3), so no action can reach a hook.
A `dry_run` request is hooked too: a `handled` reply is validated in full
(§9.2, D-d) and no action is dispatched; the reply is `HookActions {
dispatched: 0 }`.

**Action rules.**

- An empty `atm-action` list is `ATM_HOOK_MALFORMED` (D-c).
- Every action's `caller_team` must equal the registration's or
  schedule's team, and `as` must be on that team's roster. Otherwise the
  reply is `ATM_HOOK_MALFORMED` before any action runs (D-d).
- Actions run in order with no rollback. A failing action returns its
  own error code with the message prefix `hook action {i+1}/{n}
  ({command}) failed after {i} committed: ` (D-c).
- `as` lets a repo-registered hook act as another identity on the team
  (the lead). Every action is logged with the hook that requested it.

**D-b Response to the hooked client.** The client receives the response of
the last action whose `command` equals the hooked command. When no action
has the hooked command, it receives `HookActions { dispatched: n }` and the
CLI prints `hook ran N actions`.

A malformed reply, a non-zero exit, or a timeout fails the command with
`ATM_HOOK_MALFORMED`, `ATM_HOOK_EXIT` or `ATM_HOOK_TIMEOUT`, and the
transition is not written (§9.1).

**Failures always reach the caller.** Rand (2026-10-18, verbatim):
"manformed hook MUST return hook failed to caller (error)" and "a panic
at/in hook is caught and creates error return". So:

- A reply that is not exactly one of the four shapes returns
  `ATM_HOOK_MALFORMED` to the caller. It is never treated as pass-through
  or not-applicable, and the normal command never runs (§9.4).
- A python exception in the module (traceback, non-zero exit) returns
  `ATM_HOOK_EXIT` with the stderr tail.
- A panic in the daemon-side hook path (runner call, reply parse, action
  dispatch) is caught. That path runs in a spawned task, and a panicking
  `JoinError` maps to `ATM_HOOK_EXIT` with message `hook runner panicked:
  <payload>`. The daemon keeps serving.

### 2.3 Module contract (python)

The registered command is `python3 <module> <name>`, where `<name>` is one
of the eight hook names or `doctor`. The module declares:

| Symbol | Type | Use |
| --- | --- | --- |
| `TIMEOUT_MS` | `int` constant | atm's deadline for one task-hook invocation, at most `HOOK_TIMEOUT_MAX_MS` (2500, D-m). Required. |
| `CRON_TIMEOUT_MS` | `int` constant | Deadline for `cron.tick` and `cron.stop`, at most `CRON_TIMEOUT_MAX_MS` (30 000, D-q). Optional; absent means `TIMEOUT_MS`. |
| `hook(request: dict) -> dict` | function | Task hooks. |
| `cron(args: list[str]) -> dict` | function | Cron ticks (`cron.tick`). For atm-bd this runs `atm-bd cron <args>`. |
| `stop(args: list[str]) -> dict` | function | Cron stop condition (`cron.stop`), called before each scheduled run of a schedule started with `--until-hook`; replies `{"stop": true, "reason": "<text>"}` or `{"stop": false}`. For atm-bd it returns stop once the phase bead is closed. |
| `doctor(request: dict) -> dict` | function | Called by `atm doctor` from the registered team with `{schema_version, hook: "doctor", team, phase, custom}`; replies `{success, message, findings}` (§9.10). The plugin's doctor runs its own published-manifest drift check inside it. |

The module's `__main__` block is the entrypoint (§9.8): it reads stdin as
one JSON document, calls the function for `sys.argv[1]`, prints the result
with `json.dumps` and exits 0.

atm reads `TIMEOUT_MS` (and `CRON_TIMEOUT_MS` for `atm cron start`) from the file as text at registration (grammar in
§9.8); `atm hook register` refuses a module without it. It never imports
the module in-process.

**D-2 No sha in atm.** Rand (2026-10-18): "when atm-bd is published, the sha
must be calculated and stored in a manifest for every file. then the installer
can check against the published manifest to verify the file has not changed.
we should do this for the entire skill. all agents/skills/python files." and
"the plug-in can run this check as part of 'doctor' it provides and IF a hook
is installed, 'atm doctor' would call hook.doctor() when called from the team
it is registered for." So atm stores no file sha and defines no header; the
published manifest and its check live in atm-bd-orchestration (issue filed
there). `atm doctor` resolves the active hooks for the caller's team and
calls each module's `doctor(request)` with the same runner, passing team,
phase and `custom`, and prints the reply and its findings under a `hooks`
section; a missing or failing `doctor()` is reported, not
fatal (§9.10).

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

**D-10 Hooks are an input filter on one task trait.** Rand (2026-10-18,
verbatim): "I would have 'atm task' hooks implement a atm-task trait (same
trait that the normal methods implement). If hooks are installed (i.e.
there is a filter for team/phase/repo to determine if a hook-plug-in is
present) then the hook trait is called. the return array of actions are
then injectec into the normal input stream bypassing the hook-plug-in
implementation. ... it is basically between the input http stream and the
normal processing path (sort of an input filter on the commands). This
would allow expansion w/ other hook-plugin for other subcommands in the
future." and "that covers the recursion path as well. adding ATM_HOOK=1 is
a lot more logic branching that will be hard to test." So:

- `atm_core::boundary::TaskCommands` (§9.5) is one sealed async trait with
  six methods, `assign`, `start`, `close`, `move_task`, `list`, `events`.
  The method name is the hook name.
- `NormalTaskCommands` (atm-http-runtime) implements it with today's task
  path, unchanged.
- `HookedTaskCommands<D: HookActionDispatcher = UnhookedRouter> { inner:
  Arc<dyn TaskCommands>, actions: Arc<D>, snapshot, runner }` implements
  the same trait for every `D` (filter unit tests use the recording
  double).
  A method with no active registration for the caller's team calls
  `inner` directly with no spawn: a plug-in that implements two of six
  hooks passes the other four through (Rand: "you need to handle the case
  of we only implemented 2/6 in the plug-in, but that should be easy, I
  would probably just pass-through."). `Refused` returns
  `ATM_HOOK_REFUSED`, `PassThrough` calls `inner`, `NotApplicable` moves
  to the next registration (`inner` when none is left), and `Handled` sends
  each action to `actions`, the un-hooked router below the filter.
- The router's HTTP entry routes each task request to one `TaskCommands`
  method of its composed filter (§9.3). Internal callers, hook actions and
  cron actions use the base router, which has no filter.
- There is no nesting variable or header. A module returns actions instead
  of shelling out to `atm` for its own hooked command; a nested `atm` call
  from a hook process is an ordinary request bounded by the hook timeout.
- The filter selects by the caller's team and the method's hook name and
  runs the active registrations for it in order until one handles (D-5). The filter key may
  later gain repo.

Flow for a hooked task command:

1. CLI sends the command as today. The request already carries
   `caller_identity` and `caller_team`; the daemon adds its configured
   `local_host` as `caller.host` (`AtmConfig.local_host`, null when the
   operator has not set it).
2. The router classifies the request with `TaskCommand::classify` (§9.3)
   and calls that method on its filter. A request from any ingress but
   `AuthenticatedIngress::Local` goes straight to `inner`: the sending host
   ran its own filter (D-f).
3. The filter looks up the active registrations for (team, hook name) in
   its in-memory snapshot (D-8). None: `inner`, no store read, no spawn.
4. The runner spawns each registration's module in turn, oldest
   `registered_at` first, with the request JSON under the shared deadline
   of D-5.
5. Reply handling per 2.2 and D-5. The same un-hooked action loop is the sealed
   core trait `HookActionDispatcher`, which the cron crate calls (D-4).
6. One structured log entry per hook call (hook, team, phase, caller,
   duration, outcome, action count) and one per action (ADR-067).

**D-8 Active hooks are an in-memory snapshot.** The router's `HookWiring`
field `snapshot: Option<Arc<ActiveHookSnapshot>>` is `None` until
`with_hook_filter` installs a filter and its snapshot. The snapshot is loaded
once in `compose_storage_router` (`active_all()`); a load error aborts daemon
startup. Each committed `atm hook register` or `disable` runs its store call
and the snapshot refresh in the same `control_path_sync_bridge.run` closure.
A refresh failure after the commit returns daemon-unavailable "registration
committed; snapshot refresh failed; the next hooked call reloads it". An
in-flight hook call keeps its cloned registration, so a disable does not
cancel its actions. The task request path never reads `hook_registration`, so
a daemon with no registrations takes exactly today's path (ANC-014).
Registration and cron routes are accepted from `Local` ingress only, from any
roster member of the team (ADR-068 D5, Rand's round-4 ruling); no lead check.

`HookWiring` represents the construction boundary: its snapshot and runner
are both `Option` values until `with_hook_filter` installs the filter that
owns them. This permits an un-hooked router to run ordinary task bodies in
tests and composition steps. A hook-registry route on that incomplete router
fails closed with daemon-unavailable: "hook snapshot is unavailable because
hook-filter startup wiring was not installed" and recovery "Install the hook
filter during daemon startup before serving hook registry routes, then retry."
Daemon startup composes and installs the filter before it serves requests.
`with_local_host` configures the optional `local_host`; it is stamped into
`caller.host` when configured and is JSON `null` otherwise.

**D-m Budget.** `HOOK_TIMEOUT_MAX_MS = 2500` is `SERVER_REQUEST_BUDGET`
(3 s, `crates/atm-storage/src/request_budget.rs:44`) minus a 500 ms action
reserve. It applies to task hooks only, which run inside a CLI request.
§9.7 states the deadline rule. Cron runs outside any request and has its
own cap (D-q).

## 4. Storage (ADR-061 governed interface, minor bump)

Two tables, both team-scoped:

- `hook_registration(team, phase, hook, module_path, cwd, timeout_ms,
  custom, active, registered_by, registered_at, disabled_at)`; primary key
  (team, phase, hook).
- `cron_schedule(id, team, phase, name, cron_expr, module_path, cwd, args,
  timeout_ms, jitter_s, custom, active, until_at, stop_hook, created_by,
  created_at, last_run_at, last_outcome, ended_at, generation)`; unique
  on (team, phase, name); `CHECK ((active = 1) = (ended_at IS NULL))` and
  the same on `hook_registration` with `disabled_at`.

`PhaseId` is `^[a-z][a-z0-9-]{0,31}$`, checked by hand-written code with no
regex crate. `atm hook disable --phase <x>` sets `active = 0` on the
phase's active rows only, keeps an earlier `disabled_at`, and returns only
the rows it changed. Re-registering an active key replaces `module_path`,
`cwd`, `timeout_ms`, `custom` and `registered_by` and keeps
`registered_at`, so the hook keeps its chain position (D-5); re-registering
an inactive key sets `registered_at = now` and clears `disabled_at`. It is
an upsert (`ON CONFLICT DO UPDATE`), never `INSERT OR REPLACE`.
`active_all` is ordered by team, hook, parsed `registered_at`, phase, so
each key's subsequence equals `active(team, hook)`; `list` puts active
rows first, then `registered_at`, then phase. `atm cron end --phase <x>` does the same for
schedules. Starting a name that has an ended row updates that row in
place, keeps its id and adds 1 to `generation` (D-i). `record_run` and
`record_stop` take the run's `generation` and write only `WHERE id = ?
AND generation = ? AND active = 1`, so a run that outlives its slot (its
write in `spawn_blocking` survives a task abort) never writes a restarted
row. The stores read no clock: every timestamp (`registered_at`,
`disabled_at`, `created_at`, `ended_at`, run and stop times) is an
argument, and `AtmCron` stamps from its injected `Clock`. `atm hook list` and `atm cron list` read
these rows. There is no database-wide schema version constant; the proof
is a pre-bk DDL test (ADR-061 D3). The schema-reviewer records the minor
bump at plan review.

## 5. Scheduler

**D-4 Engine: its own crate behind one trait.** Rand (2026-10-18, PR
#1877): "I would expect atm cron to be in it's own crate w/ simple trait
exposing it's features." `atm cron` is the workspace crate `atm-cron`
(`crates/atm-cron`), the only implementation of the sealed core trait
`atm_core::boundary::CronScheduler` (`start`, `end`, `list`).
`AtmCron` has no other public operation besides `new` and `start_worker`;
tests fire ticks by advancing the `ManualClock` test double (feature
`test-utils`). The crate depends on `atm-core` (every store and hook type through
`atm_core::boundary`), `tokio`, `chrono`, the `cron` crate 0.17,
`async-trait`, `serde_json`, `getrandom` (for `SystemJitter`, D-r) and
`tracing`. It never depends on
`atm-storage`, `atm-http-runtime`, `atm-daemon-bootstrap`, `atm` or
`atm-storage-rusqlite`. tokio-cron-scheduler was rejected (comparison on
#1873: stale maintenance, open drift and DST bugs, 500 ms poll loop,
current-thread hang). It runs hooks through the injected `HookRunner` and
dispatches actions through the injected sealed core trait
`HookActionDispatcher`, implemented only by `UnhookedRouter`, a wrapper
over the base router with no task filter (§9.5). The bootstrap
composes `AtmCron` and installs it in the router once (§9.9). Store calls
go through one `async fn store_call<T>` that runs the sync store on
`spawn_blocking`.

**D-g Cron expressions** are the `cron` 0.17 grammar: 6 or 7 fields,
seconds first, year optional, or one of `@yearly`, `@monthly`, `@weekly`,
`@daily`, `@hourly`, in UTC. A five-field expression is refused with a
message naming the expected form. An expression with no future occurrence
is refused. Accepted: `0 * * * * *`, `0 * * * * * *`, `@daily`. Refused:
`* * * * *` (five fields), `x y`, `0 0 25 * * *` (hour 25).

Rules:

- `next` is computed from the clock after each run, after startup and
  after a reload; a missed tick is never replayed (restart, sleep, or a
  long run skips to the next future occurrence).
- One run per schedule at a time: the worker keeps `Slot { schedule, next,
  in_flight: Option<JoinHandle<CronRunOutcome>> }`. A tick that arrives
  while `in_flight` is running is skipped and logged. The loop awaits
  finished runs; `Stopped` removes the slot and any other outcome
  recomputes `next`. A reload keeps a slot only when the loaded row equals
  the slot's row on `id`, `generation`, `cron_expr`, `jitter_s`, `stop`,
  `module_path`, `cwd`, `args`, `timeout_ms` and `custom`; otherwise the
  slot is dropped, its `in_flight` aborted and a new slot built. A restart
  in place adds 1 to `generation`, so an end then start before the reload
  is never merged into the old slot, and the aborted run's write carries
  the old generation and changes nothing (§4).
- What a clock wake does is the pure `plan_step(slots, now) -> Step { run,
  skip_in_flight, stop_until, wake }`; the one `Until` rule is the pure
  `stop_due(stop, now)` used by `start`, `plan_step` and the run. Timing
  rules are tested on those functions.
- `JoinError::is_panic` of a run records `exit: hook runner panicked:
  <payload>` (generation-guarded) and the worker keeps running;
  `is_cancelled` records nothing and logs `cancelled`. An `active_all`
  failure on reload logs `reload_failed`, keeps the slots and retries at
  the next wake or bump; a failed record logs `record_failed`. A drop
  guard in the worker task sets `stopped` when the loop ends (shutdown or
  panic); then `start`, `end` and `list` return `daemon_unavailable("cron
  worker stopped")`, so `atm cron list` shows it.
- `ManualClock` (tests) counts parks: each `sleep_until` entry that finds
  `now < t`, re-parks after an `advance` included; with no wake the worker
  awaits `sleep_until(MAX_UTC)`. `advance_settled(d)` advances, then waits
  until the park count rises, which means the worker handled the advance
  and waits again. It is the quiescent barrier when no run is in flight or
  each in-flight run is held by `before_reply`; outcomes are read after
  `CronWorkerHandle::shutdown` has joined the runs.
- The worker sleeps in slices of at most `MAX_SLEEP_SLICE` (30 s) and
  checks what is due after every wake, so a wall-clock jump is seen within
  one slice.
- `record_run` updates `WHERE id = ? AND generation = ? AND active = 1`,
  so a run that finishes after `end`, or after an end and restart, does
  not overwrite the row.
- Invalid cron strings are refused at `atm cron start` (parse before
  write, in `AtmCron::start`), so the daemon never loads one. A loaded row
  with no future occurrence is ended through `record_stop` with reason
  `no future occurrence`.
- The schedule's `args` travel only inside the stdin request
  (`request = {"args": [...], "created_by": "<agent>"}`); the module's argv is
  `python3 <module> cron.tick`, the same shape as every hook.
- A cron run's reply follows 2.2: actions are dispatched un-hooked as the
  schedule's `created_by` unless an action names `as`, each with a fresh
  deadline of `SERVER_REQUEST_BUDGET`.

**D-q Cron timeout.** Rand (2026-10-18): "the cron timeout would need to
be much longer than the hook timeout. 2.5s is fine for hook. cron might
query gh, query bd, query atm aggregate information.... something more
like a 30s timeout would be more reasonable, although I expect they will
generally be much faster". `cron.tick` and `cron.stop` use the module
constant `CRON_TIMEOUT_MS`, or `TIMEOUT_MS` when it is absent, capped at
`CRON_TIMEOUT_MAX_MS = 30000`. `atm cron start` reads it client-side,
checking the grammar only (§9.8); the cron start route alone refuses a
value above the cap; it is stored as `cron_schedule.timeout_ms`. The stop check and the tick each get the
row's `timeout_ms`; each action then gets its own `SERVER_REQUEST_BUDGET`
deadline. A doctor call for a schedule uses the row's `timeout_ms`; a call
for a hook registration uses its `timeout_ms`; both are further bounded by
the 250 ms-reserved doctor budget (§9.10).

**D-r Jitter.** Rand (2026-10-18): "Random per-job offset so jobs don't all
fire on :00 <-- we should do this. it should be random over remaining
slots." The rule below is the coordinator's reading of that ruling,
stated as such; Rand may correct it.

- At `atm cron start`, `AtmCron::start` assigns the schedule a fixed
  `jitter_s` in whole seconds, before the store write, holding
  `start_lock` from the draw through the write so concurrent starts of
  one expression see each other's offset.
- `J = min(period_s / 2, 1800)`, at least 1, where `period_s` is the
  seconds between the first two occurrences after now (`J = 1` when there
  is only one).
- `jitter_s` is uniform over `[0, J)` minus the offsets already held by
  other active schedules with the same `cron_expr` (the remaining slots).
  When every offset is held it is uniform over `[0, J)`.
- It is stored in `cron_schedule.jitter_s` (`INTEGER NOT NULL DEFAULT 0`),
  so a daemon restart keeps it; a restart in place (D-i) draws again.
- Each occurrence fires at occurrence + `jitter_s`: the next fire is
  `schedule.after(&(now - jitter_s)).next() + jitter_s`, so a daemon
  restart inside an occurrence's jitter window still fires it. An `Until`
  schedule wakes at `min(next, until_at)`. `atm cron list` shows `jitter_s`.
- The random source is the injected `JitterSource` in `AtmCronDeps`
  (`SystemJitter` over the workspace's `getrandom`; `SeqJitter` in tests),
  so no test depends on chance.

**D-9 Every schedule carries exactly one stop condition.** Rand (2026-10-18,
#1873 round 4): "there should probably be a 'stop condition' which was
mandetory when it is created which would generally be a time, but a
function/hook would also be useful. for atm-bd cron, phase bead closing
would be condition for stopping." `atm cron start` requires exactly one of
`--until <rfc3339>` or `--until-hook`, stored as
`cron_schedule.until_at` (nullable text) and `cron_schedule.stop_hook`
(integer 0/1) under a CHECK that exactly one is set; in code the pair is
`StopCondition { Until { at }, Hook }` on `CronStartSpec`, `NewCronSchedule` and
`CronSchedule`. `--until` at or before now is refused by `AtmCron::start` with `ATM_CRON_EXPR_INVALID`.
The stop check is the first step of every tick:

- `Until { at: t }`: the worker wakes at `min(next, t)`; when `now >= t` the
  schedule ends (`active = 0`, `ended_at = now`,
  `last_outcome = "stopped: until reached"`) and no run happens.
- `Hook`: before the run the daemon calls `python3 <module> cron.stop` with
  the same stdin JSON shape (hook `cron.stop`, the same `request` and `custom`)
  under the schedule's `timeout_ms`. `{"stop": true, "reason": "<text>"}`
  ends the schedule with `last_outcome = "stopped: <text>"` and no run;
  `{"stop": false}` runs the tick; a malformed reply, a non-zero exit or a
  timeout is `ATM_HOOK_MALFORMED` / `ATM_HOOK_EXIT` / `ATM_HOOK_TIMEOUT`,
  the tick is skipped and logged, the schedule stays active.

`atm cron end` remains the manual stop. A schedule that has ended is listed
until its phase's rows are cleaned up, like a disabled hook. Restarting an
ended name is an UPDATE in place: the id is kept, `active = 1`, and
`ended_at`, `last_run_at` and `last_outcome` are cleared (D-i).

atm-bd usage: the phase lead starts the phase's cron with `--until-hook`
and the module's `stop()` returns `{"stop": true, "reason": "phase bead
closed"}` once the phase bead is closed; no `atm cron end` is needed at
phase close.

## 6. CLI

- `atm hook register <hook> --module <path> --phase <x> [--cwd <dir>] [--custom <json>]`
- `atm hook disable --phase <x> [<hook>]`
- `atm hook list [--phase <x>] [--json]`
- `atm cron start <name> --cron "<expr>" --module <path> --phase <x> (--until <rfc3339> | --until-hook) [--arg <v>]... [--cwd <dir>] [--custom <json>]`
  (exactly one of `--until` and `--until-hook`, D-9; `--arg` repeats, one
  value each, D-j)
- `atm cron end --phase <x> [<name>]`
- `atm cron list [--phase <x>] [--json]`
- `atm doctor`: a `hooks` section with one entry per module, cwd, team
  and phase among the caller team's active registrations and schedules,
  carrying the module's `doctor(request)` reply and findings (or its
  error); never fatal to doctor.

Rules every command follows:

- `--cwd` defaults to the module's parent directory. The client refuses a
  missing module or cwd before any request.
- `atm hook register` reads `TIMEOUT_MS` (§9.8); `atm cron start` reads
  `CRON_TIMEOUT_MS`, else `TIMEOUT_MS` (D-q). The CLI checks the grammar
  only; a value above the cap is refused by the daemon route (§9.7).
- `--custom` must parse as a JSON object; the clap error names the flag.
  `list --json` carries `custom` in full; the human table prints
  `<n> keys`. `atm cron list` also prints `JITTER_S`.
- `list --json` prints a bare JSON array of rows. The human table prints
  `none` when empty, and `STOP` as `until <UTC, seconds>` or `hook`.
- `atm hook disable` with nothing to change prints `no active hooks for
  phase <x>` and exits 0; `atm cron end` with nothing active prints `no
  active schedules for phase <x>` and exits 0.
- Exit codes follow §9.1.

Team comes from the caller context as for every atm command (ADR-066).
`atm hook register` and `atm cron start` are open to every roster member of
the team (Rand 2026-10-18 round 4: "so any roster member for both").

## 7. Sprint cut

The sprints, waves and consumed artefacts are in `phase-bk-plan.md`.

**D-7 Sprint cut by crate.** `atm-http-runtime -> atm-daemon-bootstrap`
is a forbidden edge
(`boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`), so the
runner the router calls must be a core boundary trait implemented in
bootstrap, exactly as `MessageReceivedHookSelector` is
(`crates/atm-core/src/boundary/message_received_hook_emitter.rs:54`).
The cut (plan review round 1): bk-1 the store traits, codes, constructors
and budgets (`atm-storage`, `atm-error`; the stores raise the codes, and
`atm-core` depends on `atm-storage`); bk-2 the core hook types and the four
sealed traits; bk-6 the rusqlite stores; bk-3 the runner
(`atm-daemon-bootstrap`); bk-7 the filter, dispatcher, routes and the
envelope variants with HTTP 1.13.0, because the variants and their only
exhaustive matches (`map_api_response`, `dispatch_non_write`) must change
in one layer for every layer to build; bk-4 `atm-cron`; bk-5 the CLI, the
composition, shutdown order and docs. Waves: bk-1 with bk-2; bk-6, bk-3,
bk-7; bk-4; bk-5. bk-4 must_follow bk-3 and builds on its root
`Cargo.toml`, `Cargo.lock` and `.just/lint-config.toml` edits (Rand,
2026-10-19: "do 4 waves"; this overrides `atm-beads-plan-guidelines.md:177`). bk-1 and bk-2 stay separate sprints developed
concurrently by two agents (Rand, 2026-10-19: "I would do them in parallel w/ 2 agents."; and: "that works, he starts, then when bk-1 completes, he finishes"): bk-2
codes against bk-1's atm-storage contract types from bk-1's sprint doc,
starts with bk-1, and closes only after bk-1 completes, rebased onto
bk-1's final head with its gates run on the real `atm-storage` code.

Each sprint carries its tests, never a live daemon. A test
that spawns a real child runs on the real clock with a generous
`timeout_ms` and asserts content; the timeout case uses a child that
never finishes. Scheduling is tested with `ManualClock`, and
`start_paused` only with doubles.

**D-5 Phases are independent; hooks run in order until one handles.** Rand
(2026-10-18): "phases are INDEPENDENT entities. they could have different
hook implementations." / "tasks don't need phase. arch-ctm can have task
queue w/ bj-2, bk-1, bj-4 not a big deal." / "phases look at where their
beads are". Registrations are keyed (team, phase, hook) and any number of
phases may have the same hook active at once; `atm hook register` has no
cross-phase refusal and tasks carry no phase. For a command on team T the
filter runs the active registrations of that hook for T, sequentially,
oldest `registered_at` first (phase id breaks a tie). Rand (2026-10-18):
"if you call atm task assign, it would call phase-bk-task-assign (pass-through) then phase-bj-task-assign (handle) ...". Rand's "pass-through" there is the
`not-applicable` result of the four-result ruling (§2.2). Each module
decides from its own beads whether the task is its own and replies
not-applicable for one that is not. The chain stops at the first reply
that is not not-applicable (`refused`, `pass-through` or `handled`): it
decides and later hooks are not run. When every reply is not-applicable,
`inner` runs. Actions still enter below the filter, under the
deciding registration's phase. Any hook error (`ATM_HOOK_EXIT`,
`ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED`) fails the command at once. One
further rule is a fenix decision (2026-10-18) that Rand may override:

- **One shared budget.** All hooks of one command run within the single
  request deadline. Each hook's deadline is `min(its TIMEOUT_MS, remaining
  - HOOK_ACTION_RESERVE_MS)`, computed only in `DaemonHookRunner` by the
  pure `runner_budget`. The filter passes the request deadline unchanged
  and has no budget check; a runner left without budget returns
  `ATM_HOOK_TIMEOUT` naming the phase without spawning, and the filter
  returns that error unchanged. A hook is never skipped.

Cost: up to one spawn per active phase per hooked command, so a module must
answer not-applicable quickly for a task it does not own. `atm cron start`
was already per phase: schedules are keyed (team, phase, name) and every
active schedule runs. Doctor is unchanged: one call per (module, cwd,
team, phase) (§9.10).

**D-6 `request` and action encoding.** `request` is the bare HTTP body of
the hooked command's route. An action is `{"command": "<atm command>",
"as": "<identity>", "request": <bare body>}`.
`HookAction::into_api_request` deserializes `request` with the body type
§9.2 names for `command`, checks that the body is that command (a `send`
body carries no task fields, a `task.close` body classifies as
`close`), sets `caller_identity` to `as` when present, else to `default_as` (the
original caller; whatever the body held), and returns the `ApiRequest`. The router runs it un-hooked. Registry and cron
commands and every internal route are refused with `ATM_HOOK_MALFORMED`.

## 9. Contract tables (referenced by the sprint docs)

### 9.1 Error codes

Six `AtmErrorCode` variants, all added in bk-1 (`atm-error`,
`docs/atm-error-codes.md` §5.11 and §7.1, the `is_validation` arms in
`crates/atm-storage/src/error.rs`, guidance in
`crates/atm-storage/src/error_catalog.rs`). `error_response`
(`crates/atm-http-runtime/src/message_handler.rs:775`) produces 400 for
`is_validation` and 503 otherwise; there is no 500.

| Code | Display prefix | `is_validation` | HTTP | §7.1 class | CLI exit |
| --- | --- | --- | --- | --- | --- |
| `ATM_HOOK_MODULE_INVALID` | `HOOK.MODULE_INVALID:` | true | 400 | operator_actionable | 3 |
| `ATM_CRON_EXPR_INVALID` | `CRON.EXPR_INVALID:` | true | 400 | operator_actionable | 3 |
| `ATM_HOOK_REFUSED` | none: the hook's message verbatim | true | 400 | operator_actionable | 3 |
| `ATM_HOOK_EXIT` | `HOOK.EXIT:` | false | 503 | retryable | 1 |
| `ATM_HOOK_TIMEOUT` | `HOOK.TIMEOUT:` | false | 503 | retryable | 1 |
| `ATM_HOOK_MALFORMED` | `HOOK.MALFORMED:` | false | 503 | retryable | 1 |

`ATM_HOOK_REFUSED` has empty catalog guidance, so no `Recovery:` suffix
is appended. The CLI exit arms are bk-5's
(`crates/atm/src/main.rs:106`, the three validation codes join the
`=> 3` arm; the other three take the existing `_ => 1`). Constructors on
`AtmError` (bk-1), with exact messages:

| Constructor | Message |
| --- | --- |
| `hook_exit(cause: HookExitCause)` | `Exit { code, stderr_tail }`: `HOOK.EXIT: exit <code>: <tail>`; `Signal(n)`: `HOOK.EXIT: terminated by signal <n>`; `Spawn { path, cause }`: `HOOK.EXIT: cannot run <path>: <cause>`; `Panic(payload)`: `HOOK.EXIT: hook runner panicked: <payload>`; `Missing(what)`: `HOOK.EXIT: <what>` (e.g. `python3 not found on PATH`) |
| `hook_timeout(phase: &PhaseId, detail: &str)` | `HOOK.TIMEOUT: phase <p>: <detail>`; detail is `no reply within <n> ms`, `request budget exhausted before the hook ran`, `budget exhausted before spawn` or `doctor budget exhausted` |
| `hook_malformed(detail: &str)` | `HOOK.MALFORMED: <detail>` |
| `hook_module_invalid(detail: &str)` | `HOOK.MODULE_INVALID: <detail>` |
| `hook_refused(message: &str)` | `<message>` |
| `cron_expr_invalid(detail: &str)` | `CRON.EXPR_INVALID: <detail>` |

### 9.2 Action commands

`HookAction { command: String, run_as: Option<AgentName> (serde "as"),
request: serde_json::Value }`, with `deny_unknown_fields`.
`HookAction::into_api_request(&self, team: &TeamName, default_as: &AgentName)
-> Result<ApiRequest, AtmError>` deserializes `request` as the type in this
table, checks the condition and the team field, and sets the identity field
from `as`, else from `default_as`; the body's own value never stands. Any
routed command except the hook and cron registries is allowed. Every other
outcome is `ATM_HOOK_MALFORMED`.

| `command` | `request` type | Condition | team field | identity field (`as`, else `default_as`) |
| --- | --- | --- | --- | --- |
| `send` | `WriteRequest` | no `task_id`, `task_op`, `task_complete` or `acknowledges_message_id` | `caller_team` | `caller_identity` |
| `ack` | `WriteRequest` | `acknowledges_message_id` is `Some` | `caller_team` | `caller_identity` |
| `task.assign`, `task.start`, `task.close` | `WriteRequest` | `TaskCommand::classify` yields that method | `caller_team` | `caller_identity` |
| `task.move` | `TaskMoveRequest` | - | `caller_team` | `caller_identity` |
| `task.list`, `task.events`, `task.history` | `ListQuery` | `task_ledger` is `Tasks`, `Events`, `History` | `caller_team` | `caller_identity` |
| `read` | `ReadQuery` (`Receive`) | - | `caller_team` | `caller_identity` |
| `peek` | `PeekQuery` | - | `caller_team` | `caller_identity` |
| `clear` | `ClearQuery` | - | `caller_team` | `caller_identity` |
| `search` | `SearchRequest` | `as` absent; `query.team` absent or the registration team | `query.team`, forced to the registration team | none |
| `doctor` | `DoctorQuery` | `as` absent; `all_teams` false; `team_override` absent or the registration team | `caller_team` and `team_override`, forced to the registration team | none |

| `hook.register`, `hook.disable`, `hook.list`, `cron.start`, `cron.end`, `cron.list` | - | refused |
| heartbeat, graft, reload, compatibility, queue, any other name | - | refused |

A `caller_team` other than the registration team is malformed (D-d).

### 9.3 Routing a request to a `TaskCommands` method

`TaskCommand::classify(request: ApiRequest) -> Result<TaskCommand,
ApiRequest>` (bk-2) returns the task command, or the request back when it
is not one. `TaskCommand::hook_name()` is the method name, and
`TaskCommand::request_body()` the bare body.

| Request | Method (hook) |
| --- | --- |
| `Write` with `task_op == Some(Start)` | `task.start` |
| `Write` with `task_op == Some(Close)`, or legacy `task_complete` with no `task_op` | `task.close` |
| `Write` with `task_id` `Some` and `task_op`, `task_complete`, `acknowledges_message_id` all `None` | `task.assign` |
| `Write` whose `task_op_normalized()` is `Err` | not a task command: the existing validation error surfaces |
| `Write` with `acknowledges_message_id` `Some` | not a task command |
| `TaskMove` | `task.move` |
| `Messages(List)` with `task_ledger == Some(Tasks)` | `task.list` |
| `Messages(List)` with `task_ledger == Some(Events)` | `task.events` |
| `Messages(List)` with `task_ledger == Some(History)` | not a task command (D-h) |
| everything else | not a task command |

### 9.4 Reply parsing

`HookReply::parse(stdout: &[u8]) -> Result<HookReply, AtmError>` parses one
`serde_json::Value`, refusing trailing data. `pub enum HookReply {
Handled(Vec<HookAction>), PassThrough, NotApplicable, Refused { message:
String } }` has no serde derive.

| stdout | Result |
| --- | --- |
| object, keys exactly `{result, "atm-action"}`, `result` is `"handled"`, `atm-action` a non-empty array of valid `HookAction` | `Handled` |
| object, keys exactly `{result}`, `result` is `"pass-through"` | `PassThrough` |
| object, keys exactly `{result}`, `result` is `"not-applicable"` | `NotApplicable` |
| object, keys exactly `{result, message}`, `result` is `"refused"`, `message` a non-empty string | `Refused { message }` |
| `{"result":"handled","atm-action":{}}`, `{"result":"handled","atm-action":[]}`, `{"result":"handled","atm-action":[1]}`, `{"result":"handled"}`, `{"result":"pass-through","extra":1}`, `{"result":"not-applicable","message":"x"}`, `{"result":"refused","message":""}`, `{"result":"refused"}`, `{"result":"skip"}`, `{"result":true}`, `{"success":true}`, `{"success":false,"message":"m"}`, two objects, empty, `null`, `[]`, truncated | `ATM_HOOK_MALFORMED` |

`StopReply::parse` accepts exactly `{"stop": true, "reason": "<non-empty>"}`
and `{"stop": false}`; anything else is `ATM_HOOK_MALFORMED`.

### 9.5 The task trait and the filter

```rust
// crates/atm-core/src/boundary/task_commands.rs (bk-2)
pub struct TaskCallCtx { pub caller_identity: AgentName, pub caller_team: TeamName, pub host: Option<HostName>,
                         pub ingress: AuthenticatedIngress, pub deadline: RequestDeadline, pub request_id: RequestId }
pub struct TaskAssign(pub Box<WriteRequest>);  pub struct TaskStart(pub Box<WriteRequest>);  pub struct TaskClose(pub Box<WriteRequest>);
pub struct TaskList(pub ListQuery);            pub struct TaskEvents(pub ListQuery);         // move_task takes TaskMoveRequest
#[async_trait]
pub trait TaskCommands: sealed::Sealed + Send + Sync {
    async fn assign(&self, req: TaskAssign, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn start(&self, req: TaskStart, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn close(&self, req: TaskClose, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn move_task(&self, req: TaskMoveRequest, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn list(&self, req: TaskList, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
    async fn events(&self, req: TaskEvents, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
}
pub enum TaskCommand { Assign(TaskAssign), Start(TaskStart), Close(TaskClose), Move(TaskMoveRequest), List(TaskList), Events(TaskEvents) }
impl TaskCommand {
    pub fn classify(request: ApiRequest) -> Result<Self, ApiRequest>;   // §9.3
    pub fn hook_name(&self) -> HookName;
    pub fn request_body(&self) -> serde_json::Value;
    pub async fn call(self, commands: &dyn TaskCommands, ctx: &TaskCallCtx) -> Result<ApiResponse, AtmError>;
}
```

- `NormalTaskCommands { router: StorageAndNudgeRouter }` (bk-7) holds a
  clone of the base router taken before a filter is installed, and calls
  its existing bodies: `write_unhooked` (today's `write_with_request_id`
  body from `dispatch_raw_host_qualified_local_write`, `:991`),
  `task_move` (`:608`) and `list_messages`. Behavior is unchanged; the
  existing task tests are the proof. Those three router methods are the
  single owner of the un-hooked bodies: `NormalTaskCommands`,
  `UnhookedRouter` and the `task_commands = None` path all call them.
- The router's `write_with_request_id` (`:973`), after the deadline check
  and `home_dir`/`current_dir` normalization (`:988-989`), and the
  `TaskMove` and `Messages(List)` arms of `dispatch_non_write` (`:532`)
  classify the request and call `task_commands` when it is set. When it is
  `None` they run the existing body. A `dry_run` write is classified too,
  and the hook sees `dry_run`.
- `pub struct UnhookedRouter(StorageAndNudgeRouter)` (bk-7, private
  field) is built only by `StorageAndNudgeRouter::unhooked(&self)`, a clone
  with `task_commands = None`. It is the only `HookActionDispatcher`
  besides the test double; it runs each action's `ApiRequest` through
  that entry, so no action reaches a filter. `dispatch_actions(spec:
  DispatchSpec { hook, team, phase, default_as, deadline, dry_run },
  actions)`; with `dry_run` it validates every action and dispatches none.
  A reply carries at most `HOOK_ACTIONS_MAX` = 32 actions (a longer list
  is `ATM_HOOK_MALFORMED`), which bounds one task or cron run. The three
  `HookRunner` methods stay separate: they differ in argv, stdin and the
  typed reply (D-3).

### 9.6 Plug-in coverage

A registration exists per hook name. `HookedTaskCommands` checks the
snapshot for the method's hook name before anything else, so an
unregistered method costs one map lookup. A module registered for a hook
it does not implement answers not-applicable from its `__main__` block
(§9.8).

### 9.7 Budget and runner

Constants in `atm_storage::request_budget` (bk-1): `HOOK_TIMEOUT_MAX_MS =
2500`, `CRON_TIMEOUT_MAX_MS = 30000`, `HOOK_ACTION_RESERVE_MS = 500`.

- The register route refuses `timeout_ms` above `HOOK_TIMEOUT_MAX_MS` and
  the cron start route above `CRON_TIMEOUT_MAX_MS`, each with
  `ATM_HOOK_MODULE_INVALID`. The routes are the only cap check; the CLI
  checks the grammar only (§9.8).
- `DaemonHookRunner::run` and `::stop` alone compute the runner deadline,
  with the pure `runner_budget(timeout_ms, remaining) ->
  Option<Duration>`: `min(timeout_ms, remaining - HOOK_ACTION_RESERVE_MS)`,
  `None` at or below zero, which returns `ATM_HOOK_TIMEOUT` without
  spawning. After the permit is acquired and `spec()` validates the target,
  the runner subtracts elapsed time from that budget; an exhausted remainder
  returns `ATM_HOOK_TIMEOUT` "budget exhausted before spawn" without spawning.
  The filter passes `ctx.deadline` unchanged and has no check of its own (D-5).
  `doctor` uses `doctor_budget`: `min(timeout_ms,
  remaining)` with no reserve, `None` giving `ATM_HOOK_TIMEOUT` "doctor
  budget exhausted" without spawning.
- `RequestDeadline` (bk-2) derives `PartialEq, Eq` and gains
  `remaining_ms()` (0 when expired) and `minus(Duration)`. Tests compare a
  recorded deadline by equality to the one passed, or bound it, never
  assert a time exactly.
- Task-hook actions reuse the request's remaining deadline. A cron tick or
  stop check passes `RequestDeadline::after(timeout_ms +
  HOOK_ACTION_RESERVE_MS)`, so the runner's budget is the row's
  `timeout_ms`, not the task-hook cap (D-q). Each cron action gets `now +
  SERVER_REQUEST_BUDGET`.

`DaemonHookRunner::new` does not fail. It builds the environment once with
the pure `hook_env(env: &dyn EnvSource, team, phase, windows: bool)` under
the D-1 rules and resolves `python3` (`python` with `PATHEXT` on Windows)
on that `PATH`. When a required variable or the interpreter is missing,
every call returns `hook_exit(Missing(..))`, for example `python3 not
found on PATH`. `argv[1]` is `request.hook` for `run` and `stop` and the
literal `doctor` for `doctor`; every call takes a `HookTarget` (bk-2,
`From<&HookRegistration>` and `From<&CronSchedule>`).

`process_deadline::spawn_with_deadline(spec: SpawnSpec { program, args,
env, cwd }, stdin, remaining)` builds the `tokio::process::Command`
itself and spawns it. One `timeout(remaining)` joins the stdin write and
close, the wait and both pipe reads, so a module that never reads stdin
cannot block the daemon. A `BrokenPipe` write error is ignored: a module
that exits without reading stdin is judged by its exit status and
stdout. stdout is capped at 1 MiB through a bounded `take()` read;
overflow kills the child and is `ATM_HOOK_MALFORMED`. stderr keeps only
its last 64 KiB; overflow is not an error, and the exit status decides the
result. On unix the child gets `process_group(0)` and a
timeout kills the group with `if let Some(pid) =
child.id().and_then(|id| Pid::from_raw(id as i32)) {
rustix::process::kill_process_group(pid, Signal::KILL) }` (workspace `rustix` 1.1 with `process`, already in
`Cargo.lock`, used only in `process_deadline.rs`). On Windows only the
hook process is killed and its children are not; this is a known
limitation (ADR-068 D4), there is no Windows job-object code, and
`hooks.md` documents it. On elapsed the helper then
calls `kill().await` and returns a phase-less `SpawnFailure::TimedOut {
after_ms }`, which `DaemonHookRunner` converts to `hook_timeout(phase, "no
reply within <n> ms")`. `hook_runner.rs` spawns only through this helper;
`received_hook_selector.rs` keeps its own spawn unchanged: it and
`atm-herdr/src/transport_cli.rs` kill only the leader, write no stdin and
read unbounded output, which a hook cannot use. The helper is
`spawn_until(spec, stdin, cancel)` with `spawn_with_deadline` passing
`sleep(remaining)` as `cancel`, so a test cancels on a signal instead of a
timer. A `GroupKillGuard` held until the child is reaped kills the group
on drop, so an aborted or dropped call orphans no grandchild.
`DaemonHookRunner` holds a semaphore of `HOOK_CONCURRENCY_MAX` = 16 live
hook children shared by task chains, cron runs and doctor; a permit not
acquired within the call's budget is `hook_timeout(phase, "hook
concurrency limit reached")` with no spawn.

| Failure | Code |
| --- | --- |
| spawn error, module path missing, cwd missing at run time | `ATM_HOOK_EXIT` `Spawn`: `cannot run <path>: <cause>` |
| `PATH`, `HOME` (unix), `SystemRoot`, `USERPROFILE` or the interpreter missing | `ATM_HOOK_EXIT` `Missing` |
| non-zero exit | `ATM_HOOK_EXIT` `exit <n>: <stderr tail, 512 bytes>` |
| killed by a signal | `ATM_HOOK_EXIT` `terminated by signal <n>` |
| deadline elapsed | `ATM_HOOK_TIMEOUT` |
| stdout over cap, or not a valid reply | `ATM_HOOK_MALFORMED` |
| `doctor` stdout not JSON | `ATM_HOOK_MALFORMED` |
| python exception in the module | `ATM_HOOK_EXIT` with the traceback tail |
| panic in the daemon-side hook task | `ATM_HOOK_EXIT` `hook runner panicked: <payload>` |

`HookedTaskCommands` (bk-7) runs the registration loop, each runner call,
the chain decision and the action dispatch inside one task spawned through
the router's `ChainTracker` over cloned `Arc`s; shutdown refuses new
chains, waits for live ones up to its deadline, then aborts them
(`daemon_unavailable` "hook chain cancelled: daemon shutting down"). The
snapshot holds `Arc<HookRegistration>` rows and each `HookRequest` is built
only when its hook runs. The task returns `Result<Decision, AtmError>`, with `enum
Decision { Inner, Refused(String), Done(ApiResponse) }`. The filter maps
`JoinError::is_panic()` to that error, then performs `Inner` (the `inner`
call) and `Refused` outside the spawn; `inner` is never called inside
it. The cron tick (bk-4) already runs
in a spawned task and maps a panicking `JoinError` the same way.

### 9.8 Module entrypoint and `TIMEOUT_MS`

`argv[1]` is one of `task.assign`, `task.start`, `task.close`,
`task.move`, `task.list`, `task.events`, `cron.tick`, `cron.stop`,
`doctor`. The module's `__main__` block reads stdin as one JSON document
(the §9.10 doctor request for `doctor`) and calls its handler for a `task.*` name it
implements, or prints `{"result": "not-applicable"}` for one it does not
(Rand: "I would probably just pass-through"; the result is named by the
four-result ruling, §2.2); it calls
`cron(request["request"]["args"])` for `cron.tick`,
`stop(request["request"]["args"])` for `cron.stop` and `doctor(request)`
for `doctor`. It prints the result with `json.dumps` and exits 0. `hooks.md`
carries this block in its example module, and the bk-3 fixture uses the
same block.

`read_timeout_ms(module)` accepts exactly one line at column 0 of the form
`TIMEOUT_MS = <digits>` or `TIMEOUT_MS: int = <digits>`, optionally
followed by spaces and a `#` comment. `TIMEOUT_MSX = 1` and an indented
line do not match. Zero, no matching line, or two matching lines are
`ATM_HOOK_MODULE_INVALID`. `read_cron_timeout_ms(module)` applies the same
grammar to `CRON_TIMEOUT_MS`; with no `CRON_TIMEOUT_MS` line it returns
`read_timeout_ms(module)`. Neither checks a cap: `HOOK_TIMEOUT_MAX_MS` and
`CRON_TIMEOUT_MAX_MS` are checked only by the daemon routes (§9.7).

### 9.9 Composition

- `compose_storage_router` (`crates/atm-daemon-bootstrap/src/replacement_handler.rs:268`,
  bk-5) builds the base router with
  `.with_local_host(daemon_atm_config(&env).and_then(|c| c.local_host))`
  and the snapshot load, whose error aborts startup. It then builds
  `NormalTaskCommands::new(base.clone())` and `HookedTaskCommands` over
  it with `HookedTaskCommands::new(inner, actions, snapshot, runner)`,
  `actions = Arc::new(base.unhooked())`, and installs a clone of that filter
  with `base.with_hook_filter(filter.clone())`. The composed filter is a
  field of the named `ReplacementHandler` returned by
  `build_replacement_handler` (`:204`), alongside the router,
  `RecoverySweepHandle`, runner and `UnhookedRouter`. Composition tests read
  that returned field directly; production composition has no test-only
  thread-local filter record. The runner and dispatcher fields are the same
  `Arc`s used by the filter and the cron crate. bk-5 updates all four call
  sites (`lib.rs:471`, `:1303`, `:1505`,
  `telemetry_lifecycle_tests.rs:172`).
- Router field `hooks: HookWiring`: `task_commands: Option<Arc<dyn
  TaskCommands>>`, `snapshot: Option<Arc<ActiveHookSnapshot>>` (set only by
  `with_hook_filter`, shared with the filter so register and disable refresh
  what it reads), `runner: Option<Arc<dyn HookRunner>>` (doctor only),
  `local_host: Option<HostName>`, and `cron:
  Arc<OnceLock<Arc<dyn CronScheduler>>>`, shared by clones. Unset, the cron
  routes return `daemon_unavailable("cron scheduler not installed")`.
  `install_cron_scheduler(&self, Arc<dyn CronScheduler>) -> Result<(),
  AtmError>` errors on a second call. There are no `Noop` types.
- bk-5, in `run_replacement_daemon_with_selector`
  (`crates/atm-daemon-bootstrap/src/lib.rs:445`) after
  `build_replacement_handler` (`:471`): `let cron = compose_cron(&handler,
  AtmCronDeps { dispatcher: base_dispatcher.clone() as Arc<dyn
  HookActionDispatcher>, runner: hook_runner.clone(), .. })?;`, where
  `compose_cron` builds `AtmCron` and calls `install_cron_scheduler`.
- `start_cron_worker(&cron, &mut workers)` runs after
  `start_replacement_runtime_or_abort` (`:503`) returns `Ok` and before
  `emit_ready_signal_if_requested` (`:624`). An initial `active_all` error
  aborts startup.
- `shutdown_replacement_daemon` (`:805`) calls `run_shutdown_steps` with
  the listener's finish as a future; its steps are `cron_worker`, then
  `hook_chains` (begin shutdown, drain to the deadline), then `listener`.
  The three functions live in `cron_composition.rs`. The three functions are extracted so the in-crate
  tests call them with no listener. `CronWorkerHandle::shutdown` stops scheduling,
  awaits in-flight runs until the deadline, then aborts them.

### 9.10 Doctor collection

Rand (2026-10-18): "doctor would need to know team, phase and it would
return a dict/array (whatever is consistent w/ what doctor expects)".

`doctor_hooks::collect(router, team, deadline)` (bk-7) reads the active
`hook_registration` rows and active `cron_schedule` rows of the report's
`member_roster` team (none for an all-teams report) and groups them by
`(module_path, cwd, team, phase)`. Each group is one entry naming its
`hooks` and `schedules`, and one call: `python3 <module> doctor` with
this stdin, where `custom` is the stored object of the group's first row
(registrations by hook name, then schedules by name):

```json
{ "schema_version": "1.0.0", "hook": "doctor", "team": "atm-dev",
  "phase": "bj", "custom": { "...": "the row's stored --custom object" } }
```

The module replies with one JSON object. Its findings use the fields of
atm's own `DoctorFinding` (`crates/atm-core/src/doctor/report.rs:38`):

```json
{ "success": true, "message": "manifest ok",
  "findings": [ { "severity": "warning", "code": "ATM_BD_MANIFEST_DRIFT",
                  "message": "skills/x.md differs from the published manifest",
                  "remediation": "reinstall atm-bd-orchestration" } ] }
```

- `success` (bool) and `message` (string) are required. `findings` is
  optional and defaults to `[]`.
- Each finding has `severity` (`info`, `warning` or `error`, as
  `DoctorSeverity`), `code` (string), `message` (string) and optional
  `remediation` (string or null). `code` is the module's own string, not an
  `AtmErrorCode`, since atm's closed code set cannot name a plugin's
  checks.
- Any other key, a missing required key, a bad severity or non-JSON stdout
  is `ATM_HOOK_MALFORMED`, recorded on the entry.

The entry carries `module_path`, `cwd`, `team`, `phase`, `timeout_ms`,
the reply's `success`, `message` and `findings`, or `error` when the call
fails. Every finding in the hooks section therefore carries its module and
phase. The calls run concurrently on a `tokio::task::JoinSet`. Each
call is `HookRunner::doctor(target, deadline.minus(250 ms))`, and the
runner's `doctor_budget` gives `min(timeout_ms, remaining)`, where
`timeout_ms` is the group's first row's (a registration's or a
schedule's); an exhausted budget is `ATM_HOOK_TIMEOUT` "doctor budget
exhausted" without a spawn. The section never fails doctor and never changes `summary`.
`DoctorReport.hooks` is `#[serde(default)] pub hooks:
Vec<HookDoctorEntry>` without `skip_serializing_if`, so a report with no
registration carries `"hooks": []`.

### 9.11 Routes

Six path constants in `crates/atm-core/src/api.rs` (bk-7), all `POST`:
`/v1/atm/hooks/register`, `/v1/atm/hooks/disable`, `/v1/atm/hooks/list`,
`/v1/atm/cron/start`, `/v1/atm/cron/end`, `/v1/atm/cron/list`. bk-7 adds
the `HttpRouteKind` variants, `HTTP_ROUTE_SPECS` entries 16 to 21 with
their `route_spec` arms, and arms in `route_kind_for_request`,
`encode_request_body`, `decode_success_response`,
`ApiRequest::into_inner`, `From<RequestEnvelope>` and
`crates/atm-core/src/transport/testing.rs`, and the
`decode_framework_request` (`message_handler.rs:471`) and
`map_api_response` (`:707`) arms. The `HookActions` response is sent with
header `X-ATM-Hook-Actions: <n>` and body `{"dispatched": n}`;
`decode_http_response` checks that header before
`decode_success_response`, as it does for `CLEAR_OUTCOME_HEADER`.

Request fields (bk-2, `crates/atm-core/src/hook/routes.rs`):

| Type | Fields |
| --- | --- |
| `HookRegisterRequest` | `caller_identity`, `caller_team`, `phase: PhaseId`, `hook: String`, `module_path`, `cwd`, `timeout_ms: u32`, `#[serde(default)] custom: serde_json::Value` (route: `custom_object`) |
| `HookDisableRequest` | `caller_identity`, `caller_team`, `phase: PhaseId`, `hook: Option<String>` |
| `HookListQuery` | `caller_identity`, `caller_team`, `phase: Option<PhaseId>` |
| `CronStartRequest` | `caller_identity`, `caller_team`, `phase`, `name`, `cron_expr`, `module_path`, `cwd`, `args: Vec<String>`, `timeout_ms: u32`, `stop: StopCondition`, `#[serde(default)] custom: serde_json::Value` (route: `custom_object`) |
| `CronEndRequest` | `caller_identity`, `caller_team`, `phase: PhaseId`, `name: Option<String>` |
| `CronListQuery` | `caller_identity`, `caller_team`, `phase: Option<PhaseId>` |

The router parses `hook` once with `HookName::parse_task_hook`.

## 8. Out of scope

The python module and its roles (phase bj); hooks on non-task commands;
cross-host hook execution; any replay of missed cron ticks.
