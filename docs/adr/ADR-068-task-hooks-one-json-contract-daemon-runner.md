# ADR-068 — Task Hooks: One JSON Contract Per Hook, Run By The Daemon In A Fixed Environment

| Field | Value |
| --- | --- |
| ID | ADR-068 |
| Status | Accepted |
| Date | 2026-10-18 |
| Scope | `atm task` hooks (`assign`, `start`, `close`, `move`, `list`, `events`), the hook module contract, hook registration per team and phase, the daemon-side hook runner, `atm doctor` calling `doctor()` |
| Relates to | atm-core issue #1873; `docs/plans/phase-bk/design.md` (D-1, D-2, D-3, D-5, D-6, D-7); ADR-061 (governed interfaces); ADR-066 (team from caller context, never from a daemon environment variable); ADR-067 (one structured log); ADR-069 (`atm cron`) |

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
- The three reply shapes: `{success: false, message}` refuses the call
  with the message to the caller; `{success: true}` runs the normal
  subcommand; `{success: true, atm-action: [...]}` makes atm run those
  commands instead. "5. yes 6. yes": hooks are never invoked recursively,
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
`task.events`). With no active registration for the caller's team the
subcommand runs unchanged. With one, the daemon writes a single JSON object
to the module's stdin before any storage write:

```json
{ "schema_version": "1.0.0", "hook": "task.close", "team": "atm-dev",
  "phase": "bk", "caller": { "identity": "cipher", "team": "atm-dev", "host": "mac-1" },
  "request": { "...the RequestEnvelope the CLI sent, serialized by serde..." },
  "custom": {} }
```

`request` is the serde JSON of the `RequestEnvelope` variant the CLI sent
(`crates/atm-core/src/protocol.rs`), so the hook sees exactly the fields
atm validated; `custom` passes through unchanged and atm defines nothing
inside it. The key set is the same for every hook and for `cron.tick`
(ADR-069).

### D2. Three reply shapes, actions run un-hooked, never recursively

The module prints exactly one JSON object on stdout:

- `{"success": false, "message": "<text>"}`: the subcommand is refused with
  `<text>` as the error message; atm writes nothing.
- `{"success": true}`: atm runs the normal subcommand with the original
  request.
- `{"success": true, "atm-action": [{"command": "<hook name>", "as": "<identity>", "request": {...}}, ...]}`:
  atm runs those commands in order instead of the original. `request` is a
  `RequestEnvelope` JSON; `as`, when present, replaces `caller_identity`.
  Actions are dispatched with hooks disabled, so a hook-returned command
  never consults a hook.

Anything else (empty stdout, two objects, a wrong type) is
`ATM_HOOK_MALFORMED`; a non-zero exit is `ATM_HOOK_EXIT` with the exit code
and a stderr tail; exceeding the deadline is `ATM_HOOK_TIMEOUT`. All three
fail the subcommand closed.

### D3. The module contract: `TIMEOUT_MS`, `hook`, `cron`, `stop`, `doctor`, no sha

A hook module is a Python file invoked as `python3 <module> <hook name>`
with the request on stdin and nothing else on argv. It exposes a
module-level integer constant `TIMEOUT_MS` (required), `hook(request)`,
`cron(args)` and `stop(args)` (ADR-069 D3, D4) and `doctor()`. The CLI
reads `TIMEOUT_MS` from the file as text at `atm hook register` and
`atm cron start` and sends it with the request: the file is on the
caller's machine, so that read stays CLI-side; everything else about a
registration is validated by the daemon's route, which is the only
validator of the hook name (`HookName::parse_task_hook`: the six task
names, nothing else). atm stores and compares no module sha; integrity of
the installed module is the plugin's own `doctor()` (atm-bd-orchestration
#349).

### D4. Hooks run in the daemon, in a fixed environment, under a deadline

One runner lives in `atm-daemon-bootstrap` behind the sealed core boundary
trait `atm_core::boundary::HookRunner` (the shape of
`MessageReceivedHookSelector`, since `atm-http-runtime` may not depend on
`atm-daemon-bootstrap`). The router consults it before `commit_write`, in
`task_move` and in the task-ledger list path, resolving the active hook
from an in-memory snapshot of the registrations that is replaced on every
register and disable commit, so the request path reads no store. A hooked
request is accepted from `AuthenticatedIngress::Local` only; the caller is
the request's own `caller_identity` and `caller_team`. Hook-returned
actions re-enter the router's one dispatch entry with a private
hooks-disabled flag; the same loop is the sealed core trait
`HookActionDispatcher` that the cron crate calls (ADR-069 D2). The child's environment is
exactly `ATM_HOOK=1`, `ATM_TEAM`, `ATM_PHASE`, `PATH`, `HOME`, after
`env_clear()`, with the registered cwd; the daemon's own environment never
leaks in and the daemon reads no configuration from its environment
(ADR-066). The child is killed when `TIMEOUT_MS` elapses.

### D5. Registration is per team and phase; one active row per team and hook; any roster member may register

`hook_registration` is keyed `(team, phase, hook)`; `atm hook disable
--phase <x>` sets every row of the phase inactive and keeps it for `atm hook
list`. Because a task request names a team and not a phase, at most one
registration per `(team, hook)` may be active: `atm hook register` refuses
with `ATM_HOOK_ACTIVE`, naming the other phase, until that phase is
disabled. Authority (Rand, round 4, settled): any roster member of the team
may `atm hook register` and `atm cron start`; the daemon's register and
start routes check team membership and nothing else. There is no lead
check.

### D6. `atm doctor` calls `doctor()` for the caller's team

`atm doctor` lists every active registration of the caller's team and, for
each, the module's `doctor()` reply or its typed error, through the same
runner and deadline. The section never makes doctor fatal.

### D7. One log entry per run and per action

Every hook run and every dispatched action writes one structured entry
(ADR-067) with `subsystem = "atm_hook"`, the hook, team, phase, caller,
outcome and elapsed time; no second file.

### D8. The hook request is its own versioned surface; the module is the consumer

`HOOK_SCHEMA_VERSION` (`"1.0.0"`) versions the seven top-level keys and the
reply shapes of D1 and D2 and moves independently of ADR-061's
`HTTP_API_VERSION`. `request` embeds the current `RequestEnvelope` JSON as
is: a MINOR (additive) HTTP API change may add a field inside `request`
without a hook schema bump, and the hook module is the consumer that must
tolerate additive fields there; a breaking HTTP API change to a hooked
envelope is also a breaking change of this surface and bumps
`HOOK_SCHEMA_VERSION` MAJOR under ADR-061 D3's approval gate. One test
pins the seven top-level keys and, per hook, the `request` shape.

## Consequences

- Six additive `RequestEnvelope` variants (`HookRegister`, `HookDisable`,
  `HookList`, `CronStart`, `CronEnd`, `CronList`) raise `HTTP_API_VERSION`
  one MINOR (ADR-061 D5, recorded when the bump lands).
- `hook_registration` is an additive SQLite MINOR object (ADR-061 D6 record
  of 2026-10-18); `HookRegistrationStore` is the thirteenth semantic
  storage capability, authorized by ADR-069 D5 (amending ADR-063 D3).
- Six new `AtmErrorCode` variants are registered in `atm-error-codes.md`
  before the code lands (its evolution rule).
- An action may name only one of the six task commands and its own
  envelope (`hook_name_for(request) == command`); a hook can never dispatch
  a registry, cron or other route as another identity.
- Review check: a hook consulted after a storage write, a hook consulted
  for an action, a daemon environment variable visible to the module, a
  module sha stored by atm, a store read on the no-hook request path, a
  hook-name validator outside the register route, or a lead check on
  register, is a finding.
