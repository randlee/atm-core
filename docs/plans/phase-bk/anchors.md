# Anchors - phase-bk

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bk/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Hook replaces the subcommand call; atm acts on its JSON reply | 2026-10-18 |
| ANC-002 | locked | Hooks registered per team per phase; disabled, not deleted, at phase end | 2026-10-18 |
| ANC-003 | locked | One JSON request schema with caller info for every hook | 2026-10-18 |
| ANC-004 | locked | refused reply refuses the command with the hook's message | 2026-10-18 |
| ANC-005 | locked | pass-through reply = normal subcommand behavior | 2026-10-18 |
| ANC-006 | locked | handled reply with atm-action runs the actions instead of the subcommand | 2026-10-18 |
| ANC-007 | locked | Actions are HTTP-API JSON bodies, no separate action dialect | 2026-10-18 |
| ANC-008 | locked | Hook-returned actions never trigger hooks | 2026-10-18 |
| ANC-009 | locked | An action runs as the identity named in its as field | 2026-10-18 |
| ANC-010 | locked | atm never calls bd; bead logic lives in the hook | 2026-10-18 |
| ANC-011 | locked | TIMEOUT_MS is a required module constant read at registration | 2026-10-18 |
| ANC-012 | locked | Runner enforces timeout_ms, kills the hook, fails with ATM_HOOK_TIMEOUT | 2026-10-18 |
| ANC-013 | locked | All six atm task subcommands are hookable | 2026-10-18 |
| ANC-014 | locked | Missing hook = today's behavior, no error | 2026-10-18 |
| ANC-015 | locked | atm doctor calls hook.doctor() for the registered team | 2026-10-18 |
| ANC-016 | locked | No module sha in atm; drift is the plugin doctor's job | 2026-10-18 |
| ANC-017 | locked | Cron tick runs the hook module's cron entry with args | 2026-10-18 |
| ANC-018 | locked | Cron strings live in a new atm db table | 2026-10-18 |
| ANC-019 | locked | atm cron start | end | list | 2026-10-18 |
| ANC-020 | locked | Schedules survive daemon restart | 2026-10-18 |
| ANC-021 | locked | Cron reply actions use the task-hook action path | 2026-10-18 |
| ANC-022 | locked | custom JSON is the hook's, passed through untouched | 2026-10-18 |
| REQ-BK-D1 | locked | Fixed four-variable hook environment | 2026-10-18 |
| REQ-BK-D3 | locked | One daemon-side HookRunner; the CLI spawns nothing | 2026-10-18 |
| REQ-BK-D4A | locked | cron crate, UTC, next from now, no missed-tick replay | 2026-10-18 |
| REQ-BK-D4B | locked | One in-flight run per schedule; late tick is skipped and logged | 2026-10-18 |
| REQ-BK-ERRORS | locked | Non-zero exit or malformed reply fails closed | 2026-10-18 |
| REQ-BK-LOG | locked | One ADR-067 log entry per hook call and per action | 2026-10-18 |
| REQ-BK-HOOK-CLI | locked | atm hook register | disable | list | 2026-10-18 |
| REQ-BK-SCHEMA | locked | Two new tables as one ADR-061 minor schema bump | 2026-10-18 |
| REQ-BK-TESTS | locked | Fixture module on the real clock, manual clock for scheduling, never a live daemon | 2026-10-18 |
| ANC-023 | locked | Mandatory stop condition on every cron schedule | 2026-10-18 |
| ANC-024 | locked | Stop hook ends the schedule | 2026-10-18 |
| ANC-025 | locked | Until time ends the schedule | 2026-10-18 |

## ANC-001

Anchor: An active hook replaces the atm task subcommand call: atm runs the hook first, parses its JSON reply, and the subcommand's own logic runs only when the reply is pass-through, or when every hook in the chain replies not-applicable.

> so the hooks replace the call. the hooks should return json which is processed by atm. We should be able to replace quite a few of (perhaps all) atm task subcommands.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: Hook replaces the subcommand call; atm acts on its JSON reply

Scope: adds or changes the request path of an atm task subcommand, the hook runner, or hook reply handling in atm-http-runtime or atm-daemon-bootstrap

Deterministic check to pair with the development gate: a test registers a task.assign hook whose reply is {"result": "refused", "message": "m"} and asserts the task ledger is unchanged and the command fails with ATM_HOOK_REFUSED carrying m

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-4`: the cron worker runs cron.tick hooks and replaces no atm task subcommand call
- `bk-1`: bk-1 is the two store capabilities only; replacing a subcommand call with a hook and acting on its reply is bk-3's router work
- `bk-5`: replacing the subcommand call with the hook and acting on its reply is bk-3's router; bk-5 adds the registry and cron commands and the doctor section

Fixtures:
- pass: HookedTaskCommands runs the active task.assign hook before calling inner and parses the hook's JSON reply to decide whether the normal assign logic runs.
- fail: The daemon applies the task assignment to storage first and then notifies the hook of the result for logging.

## ANC-002

Anchor: A hook registration is keyed by (team, phase, hook); disabling a phase marks every hook of that team and phase inactive instead of deleting the rows.

> 1. the skill would register the hooks as 'active' for a phase for a single team, when phase ended, hooks would be disabled.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: Hooks registered per team per phase; disabled, not deleted, at phase end

Scope: adds or changes the hook_registration table, HookRegistrationStore, atm hook register or atm hook disable

Deterministic check to pair with the development gate: hook_registration has UNIQUE(team, phase, hook) and an active column; a test disables a phase and asserts the rows remain with active = 0 and disabled_at set

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: hook_registration keying and disable are bk-1 and bk-3; bk-4 builds an in-memory HookRegistration per schedule
- `bk-2`: hook_registration keying and disable are bk-1's store; bk-2 re-exports the trait and adds the HookDisable protocol variant only
- `bk-5`: disable keeps the row in bk-1's store; bk-5's atm hook disable only sends the HookDisable envelope and prints the rows

Fixtures:
- pass: The hook_registration table is unique on (team, phase, hook); atm hook disable --phase bk sets active to 0 and disabled_at on every row of the caller's team and that phase.
- fail: Hooks are registered globally in ~/.atm.toml and apply to every team until the file is edited.

## ANC-003

Anchor: atm passes every hook one JSON request of a single published schema carrying the full subcommand request plus the caller's identity, team and host; the top-level shape is the same for every hooked command.

> 2. atm would call the hook w/ all info passed in + info of caller (who ran the hook) in a single json schema which is known and same for every command for that hook.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: One JSON request schema with caller info for every hook

Scope: adds or changes the hook request type, its serialization, or what the hook runner writes to the hook's stdin

Deterministic check to pair with the development gate: one test serializes HookRequest for all six task hooks, cron.tick and cron.stop and asserts the same top-level keys: schema_version, hook, team, phase, caller, request, custom; for each task hook request equals the bare HTTP body the CLI sends

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the HookRequest type and its one schema are bk-2; bk-1 stores registrations and schedules
- `bk-5`: the one request schema is bk-2's HookRequest; bk-5 sends no hook request

Fixtures:
- pass: HookRequest { schema_version, hook, team, phase, caller: HookCaller { identity, team, host }, request: <bare HTTP body>, custom } is written to the hook's stdin for every task hook, cron.tick and cron.stop.
- fail: Each task subcommand writes its own ad-hoc JSON to the hook, and the caller identity is passed only as an environment variable.

## ANC-004

Anchor: A refused reply must stop the subcommand before any of its logic runs, with the reply's message returned to the caller as the command error.

> if 'bd ready' false, atm-task-assign hook returns false and message to caller i.e. { success = "false" message="bead {bead-id} is not ready" }

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 4.

Summary: refused reply refuses the command with the hook's message

Scope: adds or changes hook reply parsing, the refused path, or the error output of a hooked task subcommand

Deterministic check to pair with the development gate: a hook fixture prints {"result": "refused", "message": "bead x is not ready"}; the test asserts no task row is written and the ATM_HOOK_REFUSED error carries exactly that message

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-1`: reply parsing and the refused path are bk-2 (types) and bk-3 (router); bk-1 runs nothing
- `bk-5`: the refused path is bk-3's router; bk-5 adds no task subcommand logic

Fixtures:
- pass: A reply of {"result": "refused", "message": "bead atm-bk-1 is not ready"} makes atm task assign exit non-zero with ATM_HOOK_REFUSED carrying that message and writes nothing to the task ledger.
- fail: When the hook replies refused, atm logs a warning and continues with the normal assignment.

## ANC-005

Anchor: When a hook replies pass-through, atm runs the subcommand's normal logic with the original request, giving the same result as when no hook is registered.

> or if 'bd ready' the hook either retruns { success = "true" } and atm proceeds w/ normal atm task assign logic

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: pass-through reply = normal subcommand behavior

Scope: adds or changes hook reply parsing or the pass-through path of a hooked task subcommand

Deterministic check to pair with the development gate: a test with a hook replying {"result": "pass-through"} asserts the resulting task row equals the row produced with no hook registered

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-1`: pass-through behaviour is bk-3's router path; bk-1 runs nothing
- `bk-5`: pass-through is bk-3's router path; bk-5 adds no task subcommand logic
- `bk-4`: a cron tick replaces no atm task subcommand, so there is no normal logic to pass through to; bk-4 only records a pass-through reply as the tick outcome ok (ADR-069), as ANC-001 is judged on bk-3's filter

Fixtures:
- pass: On {"result": "pass-through"} HookedTaskCommands calls inner (NormalTaskCommands) with the caller's original request unchanged.
- fail: On {"result": "pass-through"} the filter reports success to the CLI without performing the assignment.

## ANC-006

Anchor: A handled reply with an atm-action array must make atm run each action in array order instead of the original subcommand's logic.

> OR the hook returns what to do { success = "true" atm-action[] = { action-1, action-2 } } where action-1 and action-2 would be atm json commands. i.e. action-1 could be exactly the same command that was input, action-2 could be 'atm send omega-prime@hermes <escelation-message>'.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 4.

Summary: handled reply with atm-action runs the actions instead of the subcommand

Scope: adds or changes hook reply parsing or action dispatch

Deterministic check to pair with the development gate: a test with a handled reply carrying two actions asserts both ran in order and the original request was not applied

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-1`: action dispatch is bk-2's HookAction and bk-3's dispatcher; bk-1 runs nothing
- `bk-5`: action dispatch is bk-3's router; bk-5 adds no task subcommand logic

Fixtures:
- pass: On {"result": "handled", "atm-action": [...]} the filter dispatches each action in array order into the normal path below the filter (any routed command except the hook and cron registry commands) and does not run the original assign logic.
- fail: The filter runs the original assignment first and then runs the atm-action list as a follow-up.

## ANC-007

Anchor: Each atm-action entry is an atm command expressed as the same JSON body the daemon's HTTP API accepts for that command and is deserialized with that route's request type.

> 4. how I described #3 this would be true. an array of 'un-hooked' atm commands would be returned as json exactly the same as they would be passed over http.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: Actions are HTTP-API JSON bodies, no separate action dialect

Scope: adds or changes the atm-action entry type or how actions are deserialized and dispatched

Deterministic check to pair with the development gate: a test deserializes an atm-action entry's request field with the same serde type the HTTP route handler for that command uses

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the atm-action type and its parse are bk-2's HookAction and HookReply; bk-4 hands the actions to bk-2's HookActionDispatcher trait, implemented by bk-3
- `bk-1`: the atm-action entry type is bk-2's HookAction; bk-1 defines no reply type
- `bk-5`: the action entry type is bk-2's HookAction; bk-5 parses no reply

Fixtures:
- pass: An atm-action entry is {"command": "<any routed command except hook.* and cron.*>", "as": "team-lead", "request": <that route's bare HTTP body>} and is turned into the ApiRequest the HTTP handler builds for that route.
- fail: Actions are shell command strings such as "atm send omega-prime hello" that the daemon splits and executes.

## ANC-008

Anchor: atm never consults a hook while executing an action returned by a hook; every atm-action entry runs through the un-hooked command path.

> the hook would not be called recursively, so the { success = true } (pass-through case) would simply do the same thing 'atm task assign ...' would do if there was no hook.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: Hook-returned actions never trigger hooks

Scope: adds or changes action dispatch, the hook runner's entry points, or any path that runs a task subcommand on behalf of a hook

Deterministic check to pair with the development gate: a test whose task.assign hook returns an atm-action task.assign asserts the hook process was spawned exactly once

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-1`: actions enter bk-3's normal path below the hook filter; bk-1 has no dispatch path
- `bk-5`: actions enter bk-3's normal path below the hook filter; bk-5 has no dispatch path

Fixtures:
- pass: Actions returned by a hook enter the normal path below the hook filter, so a task.assign action from the task.assign hook does not spawn the hook again; there is no hooks-disabled flag, nesting variable or header.
- fail: Each atm-action entry is executed as a full CLI request, so a task.assign action consults the task.assign hook again.

## ANC-009

Anchor: An atm-action entry's as field is the identity the action runs as: atm dispatches the action with that caller identity, and an entry without as runs as the original caller.

> 6. yes

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal); point 6 of the proposal read: 'returned actions may run as the lead', 2026-10-18. Status: locked. Version 3.

Summary: An action runs as the identity named in its as field

Scope: adds or changes the atm-action entry type, action dispatch, or caller resolution for actions

Deterministic check to pair with the development gate: a test dispatches {command: task.assign, as: team-lead} from caller cipher and asserts the task row's assigner is team-lead

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the as field is bk-2's HookAction::into_api_request; bk-1 has no action type
- `bk-5`: the as field is bk-2's HookAction::into_api_request and bk-3's dispatch; bk-5 dispatches no action

Fixtures:
- pass: An action with "as": "team-lead" is dispatched with caller identity team-lead, so the resulting task assignment is recorded as assigned by team-lead.
- fail: The as field is ignored and every action runs as the identity that invoked the hooked command.

## ANC-010

Anchor: atm itself never performs a bead-tracker (bd) read or write; every bd call happens inside the hook process.

> 5. yes

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal); point 5 of the proposal read: 'bead writes stay inside the hook', 2026-10-18. Status: locked. Version 4.

Summary: atm never calls bd; bead logic lives in the hook

Scope: adds a dependency, subprocess call or storage access that touches beads, bd or .beads from any atm crate

Deterministic check to pair with the development gate: rg -n '"bd"|\.beads' crates/ matches nothing new on the sprint diff

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: bk-1 calls no process at all; the no-bd rule constrains bk-3's runner and bk-5's CLI
- `bk-2`: bk-2 calls no process at all; the no-bd rule constrains bk-3's runner and bk-5's CLI
- `bk-5`: bk-5's commands send envelopes and read TIMEOUT_MS; no bd call is possible from them, and the rule's code-level check is bk-3's

Fixtures:
- pass: The hook request carries only the bare body; every bd call happens in the hook module.
- fail: atm task assign runs bd ready itself before consulting the hook.

## ANC-011

Anchor: atm hook register must read an integer TIMEOUT_MS constant from the module's text, store it on the registration, and refuse a module that lacks one.

> 7. the timeout should probably be a required field of the hook python module i.e. const int timeout_ms = 1000.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 4.

Summary: TIMEOUT_MS is a required module constant read at registration

Scope: adds or changes atm hook register, module validation, or the timeout_ms column

Deterministic check to pair with the development gate: a test registers a module file without TIMEOUT_MS and asserts a typed refusal; with TIMEOUT_MS = 1000 the row's timeout_ms is 1000

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: TIMEOUT_MS reading and the register command are bk-5; bk-4 reads timeout_ms from the schedule row
- `bk-1`: reading TIMEOUT_MS at registration is bk-5's register command; bk-1 only stores timeout_ms with a positive CHECK
- `bk-2`: reading TIMEOUT_MS and refusing a module without it is bk-5's register command; bk-2 only carries timeout_ms in HookRegisterRequest
- `bk-3`: reading TIMEOUT_MS and refusing a module without it is bk-5's register command; bk-3's route validates the hook name only

Fixtures:
- pass: atm hook register parses TIMEOUT_MS = <int> from the module file, writes it to hook_registration.timeout_ms, and fails with ATM_HOOK_MODULE_INVALID when the constant is absent, zero, duplicated or above HOOK_TIMEOUT_MAX_MS (2500).
- fail: The hook timeout is a daemon-wide setting in ~/.atm.toml and the module declares nothing.

## ANC-012

Anchor: The hook runner must kill a hook still running at its timeout_ms deadline, at most 2500 ms for a task hook and 30000 ms for a cron tick or stop check, and fail the command with ATM_HOOK_TIMEOUT.

> This allows the hook to KNOW when it's caller will timeout so it can set a shorted timeout to complete w/out error.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal) (point 7, second sentence), 2026-10-18. Status: locked. Version 4.

Summary: Runner enforces timeout_ms, kills the hook, fails with ATM_HOOK_TIMEOUT

Scope: adds or changes the hook runner's process spawn, wait, or deadline handling

Deterministic check to pair with the development gate: a paused-clock test with a sleeping fixture hook and timeout_ms 100 asserts the child is killed and the error code is ATM_HOOK_TIMEOUT

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: deadline enforcement and the kill on timeout are bk-3's DaemonHookRunner; bk-2 defines the HookTimeout code only
- `bk-1`: deadline enforcement and the kill on timeout are bk-3's DaemonHookRunner; bk-1 spawns nothing
- `bk-5`: deadline enforcement is bk-3's DaemonHookRunner; bk-5 spawns no hook

Fixtures:
- pass: HookRunner spawns the module with kill_on_drop, waits with tokio::time::timeout(timeout_ms), kills on expiry and returns AtmErrorCode::HookTimeout.
- fail: The runner waits for the hook indefinitely and the module's TIMEOUT_MS is only advisory.

## ANC-013

Anchor: Each of the six atm task subcommands (assign, start, close, move, list, events) must have its own hook name, equal to its TaskCommands method, and consult its registered hook.

> assign,start,close are natural to override w/ hooks. we might as well hook the others

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 4.

Summary: All six atm task subcommands are hookable

Scope: adds or changes the hook name enum, the router's hook lookup, or any task subcommand's request path

Deterministic check to pair with the development gate: the hook name enum has task.assign, task.start, task.close, task.move, task.list, task.events, cron.tick and cron.stop; one test per TaskCommands method asserts that method consults only its own hook

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: hooking the six subcommands is bk-3's router consult; bk-1 only names the six hooks in HookName::TASK_HOOKS
- `bk-5`: hooking the six subcommands is bk-3's router consult; bk-5 touches no task subcommand
- `bk-4`: bk-4 adds no task subcommand hook and does not change the hook name enum or the router's hook lookup; it only consumes cron.tick and cron.stop from bk-1 and bk-2 (not_applicable 0.76 in round 1, relevant-no 0.46 in round 3)

Fixtures:
- pass: HookName covers the six task names, and HookedTaskCommands resolves the hook for each of its six methods before calling inner.
- fail: Only task.assign, task.start and task.close consult a hook; move, list and events are unhooked.

## ANC-014

Anchor: A task subcommand with no active hook for the caller's team must run its existing logic unchanged and report no hook-related warning or error.

> since all hooks would be optional (defaulting to normal behavior in the case of missing), I don't see much risk.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 5.

Summary: Missing hook = today's behavior, no error

Scope: adds or changes hook lookup or the task subcommand request paths

Deterministic check to pair with the development gate: the existing task CLI integration tests pass with no hook_registration rows and their output is unchanged; the filter's request path makes no hook_registration store call (the in-memory snapshot is the only lookup; store call counter 0 after construction)

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and the HookAction type only; refusing, passing through and dispatching are bk-3's HookedTaskCommands filter and HookActionDispatcher
- `bk-4`: no task subcommand request path is touched; the cron worker is a separate producer of hook runs
- `bk-1`: no task subcommand request path is touched by bk-1; the no-hook default is bk-3's snapshot path
- `bk-5`: the no-hook default is bk-3's snapshot path; bk-5 touches no task subcommand

Fixtures:
- pass: When no registration is active for the caller's team and hook in the filter's in-memory snapshot, HookedTaskCommands calls inner with no store read and the CLI output is unchanged from the pre-hook release.
- fail: atm task assign fails with a missing-hook error when no hook is registered for the team.

## ANC-015

Anchor: atm doctor, run for a team with an active hook registration, must invoke the module's doctor() through the hook runner and report the reply in a hooks section of the doctor report.

> the plug-in can run this check as part of 'doctor' it provides and IF a hook is installed, 'atm doctor' would call hook.doctor() when called from the team it is registered for.

Said by Rand, atm-core #1873 comment 2026-10-18T23:15Z (Rand, round 3: no sha header), 2026-10-18. Status: locked. Version 4.

Summary: atm doctor calls hook.doctor() for the registered team

Scope: adds or changes atm doctor, the doctor report sections, or doctor's use of the hook runner

Deterministic check to pair with the development gate: a test with an active registration for team T, phase P whose doctor(request) returns {"success": true, "message": "ok", "findings": [one warning]} asserts the module's stdin carried team T, phase P and the row's custom, and run_doctor for T has one hooks entry with team T, phase P, that message and that finding

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: the atm doctor hooks section is bk-5; bk-2 only declares HookRunner::doctor
- `bk-1`: the atm doctor hooks section is bk-5; HookRunner::doctor is bk-2
- `bk-3`: the atm doctor hooks section is bk-5; bk-3 implements the runner's doctor method only (ADR-068 D3 AC)

Fixtures:
- pass: atm doctor adds a hooks section with one entry per (module_path, cwd, team, phase) among the caller team's active registrations and schedules, each carrying the module's doctor(request) reply: success, message and findings in the fields of DoctorFinding.
- fail: atm doctor lists the hook_registration rows but never executes the module.

## ANC-016

Anchor: atm computes, stores and compares no sha of a hook module; module drift is detected only inside the module's own doctor() reply.

> I may have made a mistake. when atm-bd is published, the sha must be calculated and stored in a manifest for every file. then the installer can check against the published manifest to verify the file has not changed. we should do this for the entire skill. all agents/skills/python files.

Said by Rand, atm-core #1873 comment 2026-10-18T23:15Z (Rand, round 3: no sha header); withdraws the round-2 sha header. Session 2026-10-18, Rand: 'yes, drift detection becomes 100% atm-db responsibility and is done through hook.doctor', 2026-10-18. Status: locked. Version 2.

Summary: No module sha in atm; drift is the plugin doctor's job

Scope: adds a sha, hash, manifest or drift check of hook modules to any atm crate, to hook register, or to atm doctor

Deterministic check to pair with the development gate: rg -n 'sc_sha|sha256|released_sha' on the hook, cron and doctor sources matches nothing; hook_registration has no sha column

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 adds no module sha column or comparison; the anchor constrains bk-5's doctor section
- `bk-1`: bk-1 adds no module sha column or comparison; the anchor constrains bk-5's doctor section
- `bk-3`: bk-3 adds no module sha; the anchor constrains bk-5's doctor section

Fixtures:
- pass: atm hook register records module_path, cwd and timeout_ms only; atm doctor reports the module's own doctor() reply and performs no hash comparison.
- fail: atm hook register takes --released-sha and atm doctor warns when the sc-sha of the module differs from it.

## ANC-017

Anchor: A cron schedule names a hook module and its args; on each tick atm must invoke that module's cron entry with those args under the same request and reply contract as task hooks.

> b) we can register a cron function in the same python module the hook resides. cron would be registered on 'phase start' and shut-down on 'phase complete' and the method for atm-bd would simply call 'atm-bd cron <args>'.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 4.

Summary: Cron tick runs the hook module's cron entry with args

Scope: adds or changes the cron_schedule table, atm cron start, or what the cron worker executes on a tick

Deterministic check to pair with the development gate: a test with module M and args ["--phase", "bk"] asserts the tick's HookRequest has hook cron.tick and request.args equal to those args, and that the module's argv is exactly [module_path, "cron.tick"] with the args only in the stdin request

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: invoking the module's cron entry per tick is bk-4; bk-2 defines the types; bk-1 stores module_path and args only
- `bk-1`: invoking the module's cron entry per tick is bk-4; bk-1 stores module_path and args only
- `bk-3`: invoking the module's cron entry per tick is bk-4's worker; bk-3 only exposes the runner seam it uses

Fixtures:
- pass: atm cron start takes --module and --args, stores them on the schedule, and the cron worker's tick builds a cron.tick HookRequest carrying the args and runs the module through HookRunner.
- fail: The cron worker runs an arbitrary shell command string stored on the schedule.

## ANC-018

Anchor: Cron schedules are persisted as cron-expression strings in a new cron_schedule table of the atm SQLite database.

> atm cron would be a simple scheduler w/ cron strings saved in a new table in atm db.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 2.

Summary: Cron strings live in a new atm db table

Scope: adds or changes the storage schema, cron_schedule, or where schedules are persisted

Deterministic check to pair with the development gate: the migration creates cron_schedule with a cron_expr TEXT column; a test inserts and reads a schedule back through CronScheduleStore

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the cron_schedule table and its persistence are bk-1; bk-4 only consumes CronScheduleStore
- `bk-2`: the cron_schedule table and its persistence are bk-1; bk-2 re-exports the types only
- `bk-5`: the cron_schedule table is bk-1; bk-5's atm cron start only sends the CronStart envelope
- `bk-3`: the cron_schedule table is bk-1; bk-3's CronStart arm hands the row to the scheduler (ADR-069 D1 AC)

Fixtures:
- pass: A new cron_schedule table with a cron_expr column is created by the storage migration and CronScheduleStore reads and writes it.
- fail: Cron schedules are kept in a [cron] section of ~/.atm.toml and loaded at daemon boot.

## ANC-019

Anchor: The atm CLI must have the subcommands cron start, cron end and cron list: start creates an active schedule, end marks one or every schedule of a phase inactive, list prints them.

> we would have 'atm cron start', 'atm cron end', 'atm cron list'.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 3.

Summary: atm cron start | end | list

Scope: adds or changes the atm CLI's cron subcommands or their daemon routes

Deterministic check to pair with the development gate: atm cron --help lists start, end and list; a CLI test runs each against the in-process router

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the cron start, end and list subcommands and daemon routes are bk-5 and bk-3
- `bk-1`: the cron start, end and list subcommands are bk-5; bk-1 owns the CronScheduleStore only
- `bk-2`: the cron start, end and list subcommands are bk-5; bk-2 owns the protocol variants only
- `bk-3`: the atm cron subcommands are bk-5; bk-3 adds the daemon routes they call (ADR-069 D1 ACs)

Fixtures:
- pass: crates/atm/src/commands/cron.rs defines CronCommand with Start, End and List variants wired to the daemon's cron routes.
- fail: Schedules are created by editing a table with atm storage sql and there is no cron subcommand.

## ANC-020

Anchor: Every active cron_schedule row is loaded when the daemon starts, so a schedule created before a daemon restart keeps firing after the restart with no re-registration.

> cron would be persistent after restarting daemon.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 2.

Summary: Schedules survive daemon restart

Scope: adds or changes daemon boot, the cron worker's start, or how schedules are loaded

Deterministic check to pair with the development gate: a paused-clock test inserts an active schedule, starts CronWorker from the store, advances past the next fire time and asserts one hook invocation

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: restart survival is bk-1's table and bk-4's active_all load; bk-2 persists nothing
- `bk-3`: restart survival is bk-1's table and bk-4's active_all load; bk-3's cron routes call the scheduler trait only
- `bk-5`: restart survival is bk-1's table and bk-4's active_all load at worker start; bk-5 composes the worker and drives its restart test with ManualClock, so the load itself is bk-4's

Fixtures:
- pass: CronWorker::start reads every active row from CronScheduleStore at daemon boot and arms a timer for each, so no CLI call is needed after a restart.
- fail: Schedules exist only in the worker's memory; after a restart atm cron list is empty until atm cron start is run again.

## ANC-021

Anchor: A cron tick's hook reply is parsed exactly as a task hook's reply: a handled reply's atm-action list is dispatched through the same un-hooked action path, and a refused reply is recorded as the tick outcome.

> the cron design would be 'atm cron' and it would run a hook (just like the atm task hooks) at a predefined interval AND return a set of atm commands just like we described above.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal) (cron note), 2026-10-18. Status: locked. Version 3.

Summary: Cron reply actions use the task-hook action path

Scope: adds or changes cron tick handling or hook reply handling

Deterministic check to pair with the development gate: a test with a cron.tick reply carrying one send action persists a message and last_outcome == "actions: 1"

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: cron tick reply dispatch is bk-4; bk-2 defines the shared HookReply and HookAction types only
- `bk-1`: cron tick reply dispatch is bk-4 through bk-2's HookActionDispatcher; bk-1 defines no reply type
- `bk-5`: cron reply actions reach the task-hook dispatcher inside bk-4's tick; bk-5 only composes AtmCron

Fixtures:
- pass: The cron worker hands the tick's actions to the same HookActionDispatcher the task filter uses, so returned actions are dispatched identically.
- fail: The cron worker only logs the hook's stdout and never executes returned actions.

## ANC-022

Anchor: The hook request's custom field carries JSON defined by the hook's owner; atm passes it through unchanged and never validates or interprets it.

> the atm-task-close hook would accept a 'custom' json set of fields which would be defined by the custom python (not atm). atm would simply pass the json to the python function

Said by Rand, atm-bd-orchestration #340 (Rand, 2026-10-18), the origin of atm-core #1873; carried as phase-bj ANC-064 until the bj/bk split, 2026-10-18. Status: locked. Version 2.

Summary: custom JSON is the hook's, passed through untouched

Scope: adds or changes the hook request type or how custom data reaches the hook

Deterministic check to pair with the development gate: a test passes custom {"x": [1, {"y": null}]} and asserts the hook's stdin carries it byte-equal under the custom key

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the custom field is bk-2's HookRequest; bk-1 has no request type
- `bk-5`: the custom field is bk-2's HookRequest; bk-5 sends no hook request

Fixtures:
- pass: HookRequest.custom is a serde_json::Value taken from the request and written to the hook unchanged.
- fail: atm validates the custom object against its own schema and rejects unknown keys.

## REQ-BK-D1

Anchor: The hook runner must start a hook with exactly the environment ATM_TEAM, ATM_PHASE, PATH and HOME (plus SystemRoot and USERPROFILE on Windows) in the registered cwd, passing no other variable from the daemon's environment.

> The child's environment must be exactly ATM_TEAM, ATM_PHASE, PATH, HOME (plus SystemRoot and USERPROFILE on Windows) after clearing the daemon's environment, with the registered cwd.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §2.1 D-1 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 4.

Summary: Fixed four-variable hook environment

Scope: adds or changes how the hook runner spawns the hook process

Deterministic check to pair with the development gate: a fixture hook that prints its environment shows exactly those four variables on unix

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: the spawn environment is set by bk-3's DaemonHookRunner; bk-2 spawns no process
- `bk-4`: environment and cwd of the spawned hook are set by the shared DaemonHookRunner (bk-3); bk-4 reuses it
- `bk-1`: the spawn environment is set by bk-3's DaemonHookRunner; bk-1 spawns no process
- `bk-5`: the spawn environment is bk-3's DaemonHookRunner; bk-5 spawns no process

Fixtures:
- pass: DaemonHookRunner calls env_clear() and sets ATM_TEAM, ATM_PHASE, PATH and HOME, with current_dir set to the registration's cwd.
- fail: The hook inherits the daemon's full environment.

## REQ-BK-D3

Anchor: Hooks run inside the daemon process through one HookRunner shared by the task router and the cron worker; the atm CLI never spawns a hook process.

> Hooks run inside the daemon process through one HookRunner shared by the task router and the cron worker; the atm CLI never spawns a hook process.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §3 D-3 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 3.

Summary: One daemon-side HookRunner; the CLI spawns nothing

Scope: adds a hook-spawning code path in crates/atm or a second runner type

Deterministic check to pair with the development gate: crates/atm-architecture/tests/process_spawn_sites.rs pins tokio::process::Command::new to process_deadline.rs, atm-herdr transport_cli.rs and atm send_to.rs; DaemonHookRunner is constructed once in atm-daemon-bootstrap and handed to the router and to AtmCron

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the sealed HookRunner trait and the no-spawn CLI rule are bk-2 and bk-5; bk-1 is storage only

Fixtures:
- pass: DaemonHookRunner lives in atm-daemon-bootstrap and is passed by Arc to the HookedTaskCommands filter and to AtmCron; the CLI only sends the request over the socket.
- fail: The atm CLI runs the hook module itself before sending the request to the daemon.

## REQ-BK-D4A

Anchor: The cron worker parses each cron_expr with the cron crate in UTC and sleeps until the next occurrence computed from Utc::now() after every run, never replaying ticks missed while the daemon was stopped.

> The cron worker parses each cron_expr with the cron crate in UTC and sleeps until the next occurrence computed from Utc::now() after every run, never replaying ticks missed while the daemon was stopped.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §5 D-4 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 2.

Summary: cron crate, UTC, next from now, no missed-tick replay

Scope: adds or changes the cron worker's tick loop or schedule parsing

Deterministic check to pair with the development gate: a paused-clock test advances 3 intervals past a stopped period and asserts exactly one run after restart

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: cron expression parsing and next-occurrence sleeping are bk-4's worker; bk-1 only stores cron_expr
- `bk-2`: cron expression parsing and next-occurrence sleeping are bk-4's worker; bk-2 only carries cron_expr in CronStartRequest
- `bk-5`: next-occurrence computation and no-replay are bk-4's worker; bk-5 composes it only
- `bk-3`: next-occurrence computation and no-replay are bk-4's worker; bk-3 adds no cron dependency

Fixtures:
- pass: After each run CronWorker computes next = schedule.upcoming(Utc).next() and sleep_until(next); a restart after downtime fires once at the next future occurrence.
- fail: The worker uses tokio::time::interval and fires once per missed period after a stall.

## REQ-BK-D4B

Anchor: A schedule whose previous run is still in flight when its next tick is due skips that tick and logs the skip; two runs of one schedule never overlap.

> A schedule whose previous run is still in flight when its next tick is due skips that tick and logs the skip; two runs of one schedule never overlap.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §5 D-4 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 2.

Summary: One in-flight run per schedule; late tick is skipped and logged

Scope: adds or changes the cron worker's tick loop

Deterministic check to pair with the development gate: a paused-clock test with a hook slower than the interval asserts one spawn and one skip log entry

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: overlap skipping is bk-4's worker behaviour
- `bk-1`: overlap skipping is bk-4's worker behaviour
- `bk-5`: overlap skipping is bk-4's worker; bk-5 composes it only
- `bk-3`: overlap skipping is bk-4's worker

Fixtures:
- pass: CronWorker keeps an in-flight flag per schedule; a due tick while the flag is set emits a cron_tick_skipped log entry and does not spawn.
- fail: Each due tick spawns the hook regardless of whether the previous run finished.

## REQ-BK-ERRORS

Anchor: A hook that exits non-zero or replies with text that is not one of the four reply shapes fails the hooked command with ATM_HOOK_EXIT or ATM_HOOK_MALFORMED, returns that error to the caller and runs no subcommand logic.

> A hook that exits non-zero or replies with text that is not one of the four reply shapes fails the hooked command with ATM_HOOK_EXIT or ATM_HOOK_MALFORMED, returns that error to the caller and runs no subcommand logic.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §2.2 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 4.

Summary: Non-zero exit or malformed reply fails closed

Scope: adds or changes hook reply parsing or exit-status handling

Deterministic check to pair with the development gate: two tests: a fixture that exits 3 yields ATM_HOOK_EXIT; a fixture printing 'ok' yields ATM_HOOK_MALFORMED; neither writes a task row

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: bk-1 defines the six ATM_HOOK_* and cron codes (ruling A) and raises constraint errors; fail-closed reply handling is bk-2's parse and bk-3's filter
- `bk-5`: fail-closed reply handling is bk-2's parse and bk-3's runner; bk-5 only prints the error codes it receives
- `bk-2`: the requirement is the hooked command's fail-closed result (error returned to the caller, no subcommand logic run), which bk-3's runner and filter deliver and test; bk-2 supplies only HookReply::parse returning Err(ATM_HOOK_MALFORMED), which bk-3 calls

Fixtures:
- pass: The runner maps a non-zero status to AtmErrorCode::HookExit and an unparsable stdout to AtmErrorCode::HookMalformed, and the filter returns that error without calling inner.
- fail: A hook that prints nothing is treated as pass-through.

## REQ-BK-LOG

Anchor: Every hook call and every hook-returned action must emit one structured log entry carrying team, phase, hook, outcome and elapsed milliseconds (ADR-067).

> Every hook call and every hook-returned action emits one structured log entry carrying team, phase, hook, outcome and elapsed milliseconds (ADR-067).

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §3; ADR-067 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 4.

Summary: One ADR-067 log entry per hook call and per action

Scope: adds or changes the hook runner, action dispatch, or their tracing

Deterministic check to pair with the development gate: a tracing-capture test asserts one action=run entry and one action=action entry per action with the five fields

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: structured log entries per hook run and action are emitted by bk-3's runner and router; bk-2 runs nothing
- `bk-1`: structured log entries per hook run and action are emitted by bk-3's runner and router and bk-4's worker; bk-1 runs nothing
- `bk-5`: the per-run and per-action log entries are bk-3's runner and router and bk-4's worker; bk-5 logs nothing new

Fixtures:
- pass: HookedTaskCommands emits tracing::info!(subsystem = "atm_hook", action = "run", team, phase, hook, outcome, elapsed_ms) once per call, and the HookActionDispatcher emits one action = "action" entry per action; atm-cron emits subsystem = "atm_cron" tick and stop_check entries.
- fail: Hook calls are logged only on failure.

## REQ-BK-HOOK-CLI

Anchor: The atm CLI must have the subcommands hook register <hook> --module <path> --phase <x> [--cwd <dir>], hook disable --phase <x> [<hook>] and hook list [--phase <x>] [--json].

> The atm CLI has the subcommands hook register <hook> --module <path> --phase <x> [--cwd <dir>], hook disable --phase <x> [<hook>] and hook list [--phase <x>] [--json].

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §6 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 3.

Summary: atm hook register | disable | list

Scope: adds or changes the atm CLI's hook subcommands or their daemon routes

Deterministic check to pair with the development gate: atm hook --help lists register, disable and list with those flags; a CLI test runs each against the in-process router

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: the hook register, disable and list CLI is bk-5; bk-2 adds only the protocol variants
- `bk-1`: the hook register, disable and list CLI is bk-5; bk-1 adds the store behind it only
- `bk-3`: the atm hook subcommands are bk-5; bk-3 adds the daemon routes and the hook-name validation they rely on (ADR-068 D3 AC)

Fixtures:
- pass: crates/atm/src/commands/hook.rs defines HookCommand with Register, Disable and List variants carrying --module, --phase, --cwd and --json.
- fail: Hooks are registered by hand-editing a table and there is no atm hook subcommand.

## REQ-BK-SCHEMA

Anchor: The hook_registration and cron_schedule tables are added in one ADR-061 minor bump of the storage schema version, applied by the existing migration path in atm-storage-rusqlite.

> The hook_registration and cron_schedule tables are added in one ADR-061 minor bump of the storage schema version, applied by the existing migration path in atm-storage-rusqlite.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §4; ADR-061 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 4.

Summary: Two new tables as one ADR-061 minor schema bump

Scope: adds or changes the storage schema or its migrations

Deterministic check to pair with the development gate: crates/atm-storage-rusqlite/tests/hook_schema.rs::pre_bk_ddl_set_reads_and_writes_after_hook_tables_created: both tables are created by ensure_schema on an empty database and on one built from the pre-bk DDL, and the pre-bk statements still read and write

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the storage schema change and the pre-bk DDL proof are bk-1; this sprint edits no storage crate
- `bk-2`: the storage schema change and the pre-bk DDL proof are bk-1; bk-2 edits no storage crate
- `bk-5`: the storage schema change is bk-1; bk-5 edits no storage crate
- `bk-3`: the storage schema change is bk-1; bk-3 edits no storage crate

Fixtures:
- pass: The existing ensure_schema migration path in atm-storage-rusqlite creates hook_registration and cron_schedule together and idempotently, and ADR-061's D6 record of 2026-10-18 lists both tables as one MINOR storage schema change.
- fail: Each sprint adds its own table with its own schema-version bump.

## REQ-BK-TESTS

Anchor: Hook execution is tested with a fixture hook module on the real clock, the timeout case using a child that never finishes, and cron scheduling with a manual test clock; no test starts, targets or depends on a live daemon.

> Each sprint carries its tests, never a live daemon. A test that spawns a real child runs on the real clock with a generous `timeout_ms` and asserts content; the timeout case uses a child that never finishes. Scheduling is tested with `ManualClock`, and `start_paused` only with doubles.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §7 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 4.

Summary: Fixture module on the real clock, manual clock for scheduling, never a live daemon

Scope: adds tests for hooks, cron, or doctor's hook section

Deterministic check to pair with the development gate: rg -n 'atm daemon|ATM_DAEMON|spawn.*daemon' on the new tests matches nothing; no test that spawns fixture_hook.py uses start_paused; atm-cron scheduling tests advance ManualClock

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: bk-1's tests are store round trips on a temp database; no hook module runs and no clock is read
- `bk-2`: bk-2's tests are serde and parse unit tests; no hook module runs and no clock is read

Fixtures:
- pass: Tests in crates/atm-daemon-bootstrap/src/hook_runner.rs spawn fixture_hook.py under #[tokio::test] on the real clock with timeout_ms = 30000, the timeout case uses a child that never finishes against timeout_ms = 200, and atm-cron's scheduling tests advance a ManualClock.
- fail: The integration test starts a daemon on the default socket and registers a hook against it.

## ANC-023

Anchor: Every cron schedule must be created with exactly one stop condition, a time (--until) or a stop hook (--until-hook); atm cron start must refuse a schedule with none or both, and the stored row must carry the condition.

> there should probably be a 'stop condition' which was mandetory when it is created which would generally be a time, but a function/hook would also be useful.

Said by Rand, atm-core #1873 comment 6257077671 (Rand, round 4, 2026-10-18), relayed on PR #1877, 2026-10-18. Status: locked. Version 2.

Summary: Mandatory stop condition on every cron schedule

Scope: adds or changes atm cron start, NewCronSchedule, CronStartRequest, or the cron_schedule columns

Deterministic check to pair with the development gate: a CLI test runs atm cron start without a stop flag and with both flags and asserts a usage error and no request; the cron_schedule CHECK rejects a row with neither until_at nor stop_hook and one with both

Excluded from (not judged; the cell passes as `excluded`):
- `bk-3`: the stop condition is required by bk-2's type, stored by bk-1, enforced by bk-4 and bk-5; bk-3's CronStart arm passes it through unchanged (ADR-069 D4 AC)

Fixtures:
- pass: atm cron start requires exactly one of --until <rfc3339> or --until-hook and exits with a usage error, sending no request, when neither or both are given; NewCronSchedule carries a StopCondition enum (Until or Hook) and the cron_schedule CHECK rejects a row with neither or both.
- fail: atm cron start takes a cron expression and a module; a stop time can be passed optionally, and a schedule without one runs until atm cron end is called.

## ANC-024

Anchor: A schedule started with --until-hook must have the daemon call the module's stop entry (hook name cron.stop, same stdin shape) before each scheduled run; a reply of stop true with a reason must end the schedule with that reason recorded and run nothing, stop false must run the tick, and a malformed reply must skip the tick and leave the schedule active.

> a function/hook would also be useful. for atm-bd cron, phase bead closing would be condition for stopping.

Said by Rand, atm-core #1873 comment 6257077671 (Rand, round 4, 2026-10-18), relayed on PR #1877, 2026-10-18. Status: locked. Version 2.

Summary: Stop hook ends the schedule

Scope: adds or changes the cron tick sequence, the cron.stop request, StopReply, or record_stop

Deterministic check to pair with the development gate: a paused-clock test with DummyHookRunner stop replies {stop:true,reason:r}, {stop:false} and a malformed reply asserts respectively: row ended with last_outcome stopped: r and no cron.tick call; one cron.stop then one cron.tick call; row still active and no cron.tick call

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the stop-hook call and its outcome handling are bk-4's tick sequence; bk-1 provides record_stop only, judged under ANC-023
- `bk-2`: bk-2 defines StopReply::parse only (judged by its own AC under ANC-024's shapes); the stop call before each run and the row update are bk-4's tick sequence
- `bk-3`: the stop call before each run and the row update are bk-4's tick sequence; bk-3 implements DaemonHookRunner::stop only (its own ADR-069 D4 AC)

Fixtures:
- pass: For a schedule with the Hook stop condition, each tick first calls the module's stop entry as cron.stop with the same stdin shape; a reply {stop: true, reason} ends the schedule with last_outcome stopped: <reason> and runs nothing, {stop: false} runs cron.tick, and a malformed reply skips the tick and leaves the schedule active.
- fail: A schedule with the stop hook runs cron.tick on every fire time; after the tick the worker calls the stop entry and logs its reply, and a malformed reply ends the schedule.

## ANC-025

Anchor: A schedule started with --until <time> must end when that time is reached: the row is set inactive with ended_at and last_outcome stopped: until reached, and no run happens at or after that time.

> which would generally be a time

Said by Rand, atm-core #1873 comment 6257077671 (Rand, round 4, 2026-10-18), relayed on PR #1877, 2026-10-18. Status: locked. Version 2.

Summary: Until time ends the schedule

Scope: adds or changes the cron tick sequence, the worker wake time, or until_at handling

Deterministic check to pair with the development gate: a paused-clock test with until_at = now + 90 s on an every-minute schedule asserts one runner call at 60 s, the row ended at 90 s with last_outcome stopped: until reached, and no call at 120 s

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the until wake and stop are bk-4's tick sequence; bk-1 provides the until_at column and record_stop only, judged under ANC-023
- `bk-2`: the until wake and stop are bk-4's tick sequence; bk-2 only carries StopCondition in CronStartRequest
- `bk-5`: the until wake and stop are bk-4's tick sequence; bk-5 stores the flag only (judged under ANC-023)
- `bk-3`: the until wake and stop are bk-4's tick sequence; bk-3 passes the stop condition through CronStart only (judged under ANC-023)

Fixtures:
- pass: The worker wakes at the earlier of the next fire time and until_at; when until_at is reached it calls record_stop with reason until reached, so the row is inactive with ended_at set, and no run happens at or after that time.
- fail: until_at is shown in atm cron list for the operator's information; the worker keeps firing on schedule until someone runs atm cron end.
