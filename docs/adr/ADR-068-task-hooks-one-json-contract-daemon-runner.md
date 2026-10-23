# ADR-068 — Task Hooks: One JSON Contract Per Hook, Run By The Daemon In A Fixed Environment

| Field | Value |
| --- | --- |
| ID | ADR-068 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | `atm task` hooks (`assign`, `start`, `close`, `move`, `list`, `events`) as a filter on the `TaskCommands` trait, the hook module contract, hook registration per team and phase, the daemon-side hook runner, `atm doctor` calling `doctor()` |
| Relates to | atm-core issue #1873; `docs/plans/phase-bk/design.md` (D-1, D-2, D-3, D-5, D-6, D-7, D-10); ADR-061 (governed interfaces); ADR-066 (team from caller context, never from a daemon environment variable); ADR-067 (one structured log); ADR-069 (`atm cron`) |

## Context

Phase bj's orchestration (atm-bd-orchestration) needs `atm task close` to
run a sanity check before the close is admitted, and the lead's dispatch
to be replaceable by a script. The first proposal was a post-hoc hook that
observed the subcommand. Rand's rulings on #1873 (2026-10-18, verbatim):

- "so the hooks replace the call. the hooks should return json which is
  processed by atm. We should be able to replace quite a few of (perhaps
  all) atm task subcommands."
- "1. the skill would register the hooks as 'active' for a phase for a
  single team, when phase ended, hooks would be disabled. 2. atm would
  call the hook w/ all info passed in + info of caller (who ran the hook)
  in a single json schema which is known and same for every command for
  that hook. the environment should probably also be known/consistent
  (but likely redundant w/ the json)."
- The reply results, as amended by Rand (2026-10-18): "we should add a 3-rd return type. instead of success=true/false which signals pass-through, we could return enum with { handled => run my actions, not-applicable, pass-through }".
  `{result: "handled", atm-action: [...]}` makes atm run those commands
  instead; `{result: "pass-through"}` runs the normal subcommand;
  `{result: "not-applicable"}` hands the call to the next hook;
  `{result: "refused", message}` refuses the call with the message to the
  caller. "5. yes 6. yes": hooks are never invoked recursively,
  and a returned action may run as another identity (the lead).
- "assign,start,close are natural to override w/ hooks. we might as well
  hook the others and since all hooks would be optional (defaulting to
  normal behavior in the case of missing), I don't see much risk." The
  module carries a `TIMEOUT_MS` constant and a `doctor` entry that
  `atm doctor` picks up.
- Round 3: "I may have made a mistake. when atm-bd is published, the sha
  must be calculated and stored in a manifest for every file." and "the
  plug-in can run this check as part of 'doctor' it provides and IF a hook
  is installed, 'atm doctor' would call hook.doctor() when called from the
  team it is registered for." No sha header in the module, no sha in atm.
- Round 4 (#1873 comment 6257077671): "atm hook register would be done by
  phase-lead, which is not necessarily team-lead. I would think any roster
  member can register the hooks. atm cron start basically allows anything
  to get registeted, from a general sense any agent could set up a cron.
  for atm-bd, phase-lead would set up cron job gated by the phase. there
  should probably be a 'stop condition' which was mandetory when it is
  created which would generally be a time, but a function/hook would also
  be useful. for atm-bd cron, phase bead closing would be condition for
  stopping." and "so any roster member for both".

The design (`docs/plans/phase-bk/design.md`) resolves where the hook runs:
the CLI is a thin client (ADR-ATM-001) and the daemon owns every task
write, so the hook must run before the daemon's write and from one place.

## Decision

### D1. The hook replaces the subcommand, through one request schema

Each of the six `atm task` subcommands has one optional hook name
(`task.assign`, `task.start`, `task.close`, `task.move`, `task.list`,
`task.events`); `cron.tick` and `cron.stop` are the scheduler's (ADR-069).
`atm task history` is not hooked. With no active registration for the
caller's team the subcommand runs unchanged. With one or more (one per
phase, D5), the daemon writes a single JSON object to each module's stdin
before any storage write:

```json
{ "schema_version": "1.0.0", "hook": "task.close", "team": "atm-dev",
  "phase": "bk", "caller": { "identity": "cipher", "team": "atm-dev", "host": "mac-1" },
  "request": { "...the bare HTTP request body the CLI sent..." },
  "custom": { "goal": "..." } }
```

`request` is the bare HTTP body of the command
(`TaskCommand::request_body`, the bytes `encode_request_body` in
`crates/atm-core/src/api.rs` writes), so the hook sees exactly the fields
atm validated. `custom` is the JSON object given with `--custom` at
`atm hook register` (or `atm cron start`), stored with the row and passed
unchanged; atm never reads a key inside it, refuses a non-object or more
than 65 536 serialized bytes at the route, and sends `{}` when none was
given. Rand (2026-10-18): "I think we will want custom json because we
don't need to write a python module for every agent/team/repo that want
the same thing." The key set is the same for every hook.

### D2. Four reply results; actions enter the normal path below the filter

The module prints exactly one JSON object on stdout:

- `{"result": "refused", "message": "<text>"}`: the subcommand is refused
  with `ATM_HOOK_REFUSED` carrying `<text>`; atm writes nothing, and the
  chain stops.
- `{"result": "pass-through"}`: the hook owns the task; atm runs the
  normal subcommand with the original request, and the chain stops.
- `{"result": "not-applicable"}`: not this hook's task; the next hook in
  the chain (D5) runs, and when every hook is not-applicable atm runs the
  normal subcommand.
- `{"result": "handled", "atm-action": [{"command": "<name>", "as": "<identity>", "request": {...}}, ...]}`:
  atm runs those commands in order instead of the original. `command` is
  any atm command with an HTTP route except `hook.register`,
  `hook.disable`, `hook.list`, `cron.start`, `cron.end` and `cron.list`;
  `request` is that route's bare body; `caller_identity` is `as` when
  present, else the original caller, whatever the body held. Every action's team must equal the registration's
  team and `as` must be on the roster, else the reply is malformed and no
  action runs. An empty list is malformed. A failing action stops the
  list with no rollback, and the error names how many actions committed.
  The caller receives the last action's response for the hooked command,
  or a count of the actions run.

Actions are dispatched into the normal input stream below the filter, so
a hook-returned command never consults a hook.

Failures always reach the caller as errors. Rand (2026-10-18): "manformed
hook MUST return hook failed to caller (error)". Any stdout that is not
exactly one of the four shapes, the earlier `success` key included, is `ATM_HOOK_MALFORMED` and never falls
through to the normal command. A non-zero exit, including a Python
exception, is `ATM_HOOK_EXIT` with the exit code and a stderr tail.
Exceeding the deadline is `ATM_HOOK_TIMEOUT`. Rand: "a panic at/in hook is
caught and creates error return". A panic in the daemon-side hook task is
caught from its `JoinError` and returned as `ATM_HOOK_EXIT` "hook runner
panicked: <payload>", and the daemon keeps serving.

### D3. The module contract: `TIMEOUT_MS`, a `__main__` dispatch, `cron`, `stop`, `doctor`, no sha

A hook module is a Python file invoked as `python3 <module> <hook name>`
(`python` on Windows) with the request on stdin and nothing else on argv.
It declares a module-level `TIMEOUT_MS` (required, at most
`HOOK_TIMEOUT_MAX_MS` = 2500, task hooks only) and may declare
`CRON_TIMEOUT_MS` for cron runs (at most 30 000, ADR-069 D3). Its `__main__` block calls the module's
handler for a `task.*` name it implements and prints `{"result": "not-applicable"}`
for one it does not, so a module that implements two of six hooks lets
the other four through (Rand: "I would probably just pass-through"). It
calls `cron(args)` and `stop(args)` (ADR-069 D3, D4) and `doctor(request)`
(D6).

The CLI reads `TIMEOUT_MS` from the file as text at `atm hook register`,
and `CRON_TIMEOUT_MS` (else `TIMEOUT_MS`) at `atm cron start`, checking
the grammar only, and sends it with the request. The file is on the
caller's machine, so that read stays CLI-side. Everything else about a
registration, including the timeout caps, is validated by the daemon's
route, which is the only validator of the hook name and the only cap
check. atm stores and compares no module sha;
integrity of the installed module is the plugin's own `doctor()`
(atm-bd-orchestration #349).

### D4. Hooks are an input filter on one task trait, run in the daemon in a fixed environment

Rand (2026-10-18, verbatim): "I would have 'atm task' hooks implement a
atm-task trait (same trait that the normal methods implement). If hooks
are installed (i.e. there is a filter for team/phase/repo to determine if
a hook-plug-in is present) then the hook trait is called. the return array
of actions are then injectec into the normal input stream bypassing the
hook-plug-in implementation." and "that covers the recursion path as well.
adding ATM_HOOK=1 is a lot more logic branching that will be hard to
test."

- Hooks are an input filter over the one sealed task trait
  `atm_core::boundary::TaskCommands`: the filter and today's task path
  implement the same trait, and the filter sits between local HTTP input
  and the normal path.
- A hook's returned actions re-enter below the filter, so they never reach
  a hook. There is no nesting variable or header.
- The daemon runs the hook module, through one runner behind a sealed core
  trait, in a fixed, cleared environment and the registered cwd.
- On Unix the production runner kills the hook's process group on timeout,
  cancellation, output overflow, or drop. Windows has no job-object
  implementation here, so `kill_on_drop` and the explicit child kill
  terminate only the direct hook process; its descendants can outlive the hook
  invocation.

The trait methods, types, environment, budget arithmetic and platform
notes are in `docs/plans/phase-bk/design.md` §9.5 and §9.7.

### D5. Registration is per team and phase; phases are independent; any roster member may register

`hook_registration` is keyed `(team, phase, hook)`; re-registering an active key replaces its module,
cwd, timeout, custom and registrant and keeps `registered_at` (its chain
position), while re-registering an inactive key sets `registered_at` to
now (an upsert, never a replace); `atm hook disable
--phase <x>` sets every row of the phase inactive and keeps it for `atm hook
list`. Rand (2026-10-18): "phases are INDEPENDENT entities. they could have
different hook implementations." / "tasks don't need phase. arch-ctm can
have task queue w/ bj-2, bk-1, bj-4 not a big deal." / "phases look at where
their beads are". Any number of phases may have the same hook active, and
tasks carry no phase. Rand (2026-10-18): "if you call atm task assign, it would call phase-bk-task-assign (pass-through) then phase-bj-task-assign (handle) ..." A hooked command runs the
active registrations of its hook for the team, sequentially, oldest
`registered_at` first. Rand's "pass-through" there is the not-applicable
result of D2: each module replies not-applicable for a task that is not
its own. The chain stops at the first reply that is not not-applicable,
which decides; later hooks are not run. All not-applicable runs the normal
command. One design decision (fenix, 2026-10-18, open to Rand):
all hooks of one command share the one request deadline, each getting
`min(TIMEOUT_MS, remaining - 500 ms)`, and a hook left without budget fails
the command with the runner's `ATM_HOOK_TIMEOUT` naming its phase. The cost is up to one spawn per active phase per
hooked command. Authority (Rand, round 4, settled): any roster member of
the team may `atm hook register` and `atm cron start`; the daemon's
register and start routes check team membership and nothing else. There is
no lead check.

### D6. `atm doctor` calls `doctor()` for the caller's team

Rand (2026-10-18): "the plug-in can run this check as part of 'doctor'
it provides and IF a hook is installed, 'atm doctor' would call
hook.doctor() when called from the team it is registered for." and
"doctor would need to know team, phase and it would return a dict/array
(whatever is consistent w/ what doctor expects)". `atm doctor` calls each
registered module's `doctor` with the team and phase, and reports its
findings in atm's own finding shape. It never makes doctor fatal. The
request and reply JSON are in `design.md` §9.10.

### D7. One log entry per run and per action

Every hook run and every dispatched action writes one structured entry
(ADR-067) with `subsystem = "atm_hook"`, the hook, team, phase, caller,
outcome and elapsed time; no second file.

### D8. The hook request is its own versioned surface; the module is the consumer

The hook request and reply are a contract consumed by modules outside atm
(atm-bd's hooks, phase bj). `HOOK_SCHEMA_VERSION` (`"1.0.0"`) versions it under
ADR-061: an additive change is MINOR and a breaking change is MAJOR.

## Consequences

- Six additive `RequestEnvelope` variants (`HookRegister`, `HookDisable`,
  `HookList`, `CronStart`, `CronEnd`, `CronList`), one `HookActions`
  response and the `X-ATM-Hook-Actions` header raise `HTTP_API_VERSION`
  one MINOR to `1.13.0` (ADR-061 D5, recorded when the bump lands).
- `hook_registration` is an additive SQLite MINOR object (ADR-061 D6 record
  of 2026-10-18); `HookRegistrationStore` is the thirteenth semantic
  storage capability, authorized by ADR-069 D5 (amending ADR-063 D3).
- Six new `AtmErrorCode` variants (`ATM_HOOK_MODULE_INVALID`,
  `ATM_HOOK_REFUSED`, `ATM_HOOK_EXIT`,
  `ATM_HOOK_TIMEOUT`, `ATM_HOOK_MALFORMED`, `ATM_CRON_EXPR_INVALID`) are
  registered in `atm-error-codes.md` in the sprint that adds them.
- An action may name any routed command except the hook and cron registry
  routes, within the registration's team.

## BK-9 amendment — cron replies and mutation JSON

Task `HookReply` remains an exact four-result parser. Cron uses a separate
`CronHookRunner` and the versioned cron-only reply contract; an optional
`cron-output` is never accepted on a task reply. The runner clears the child
environment and supplies cron's fixed identity/team only for `cron.tick` and
`cron.stop`; that environment is not sender authority.

The hook mutation CLI JSON contract is fixed before downstream implementation:
register emits `action` plus the eleven-field persisted row; disable emits
`action`, disabled rows and a count equal to array length, including successful
zero matches. Existing human output, typed failures, and bare list JSON remain
unchanged.
