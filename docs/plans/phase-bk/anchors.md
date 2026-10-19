# Anchors - phase-bk

Constraints captured during planning. Each anchor is a testable statement written by the orchestrator and approved by the user; the user's verbatim words are kept beneath it as provenance. Every anchor is judged at the planning gate and again at the development gate. Edit `anchors.json`, not this file; this file is rendered.

Intent: `docs/plans/phase-bk/intent.md`

| ID | Status | Summary | Stated |
| --- | --- | --- | --- |
| ANC-001 | locked | Hook replaces the subcommand call; atm acts on its JSON reply | 2026-10-18 |
| ANC-002 | locked | Hooks registered per team per phase; disabled, not deleted, at phase end | 2026-10-18 |
| ANC-003 | locked | One JSON request schema with caller info for every hook | 2026-10-18 |
| ANC-004 | locked | success false reply refuses the command with the hook's message | 2026-10-18 |
| ANC-005 | locked | success true without actions = normal subcommand behavior | 2026-10-18 |
| ANC-006 | locked | success true with atm-action runs the actions instead of the subcommand | 2026-10-18 |
| ANC-007 | locked | Actions are HTTP-API JSON bodies, no separate action dialect | 2026-10-18 |
| ANC-008 | locked | Hook-returned actions never trigger hooks | 2026-10-18 |
| ANC-009 | locked | An action runs as the identity named in its as field | 2026-10-18 |
| ANC-010 | locked | atm never calls bd; bead logic lives in the hook | 2026-10-18 |
| ANC-011 | locked | TIMEOUT_MS is a required module constant read at registration | 2026-10-18 |
| ANC-012 | locked | Runner enforces timeout_ms, kills the hook, fails with HOOK.TIMEOUT | 2026-10-18 |
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
| REQ-BK-D1 | locked | Fixed five-variable hook environment | 2026-10-18 |
| REQ-BK-D3 | locked | One daemon-side HookRunner; the CLI spawns nothing | 2026-10-18 |
| REQ-BK-D4A | locked | cron crate, UTC, next from now, no missed-tick replay | 2026-10-18 |
| REQ-BK-D4B | locked | One in-flight run per schedule; late tick is skipped and logged | 2026-10-18 |
| REQ-BK-ERRORS | locked | Non-zero exit or malformed reply fails closed | 2026-10-18 |
| REQ-BK-LOG | locked | One ADR-067 log entry per hook call and per action | 2026-10-18 |
| REQ-BK-HOOK-CLI | locked | atm hook register | disable | list | 2026-10-18 |
| REQ-BK-SCHEMA | locked | Two new tables as one ADR-061 minor schema bump | 2026-10-18 |
| REQ-BK-TESTS | locked | Tests use a fixture module and paused clock, never a live daemon | 2026-10-18 |
| ANC-023 | locked | Mandatory stop condition on every cron schedule | 2026-10-18 |
| ANC-024 | locked | Stop hook ends the schedule | 2026-10-18 |
| ANC-025 | locked | Until time ends the schedule | 2026-10-18 |

## ANC-001

Anchor: An active hook replaces the atm task subcommand call: atm runs the hook first, parses its JSON reply, and the subcommand's own logic runs only when the reply is pass-through.

> so the hooks replace the call. the hooks should return json which is processed by atm. We should be able to replace quite a few of (perhaps all) atm task subcommands.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: Hook replaces the subcommand call; atm acts on its JSON reply

Scope: adds or changes the request path of an atm task subcommand, the hook runner, or hook reply handling in atm-http-runtime or atm-daemon-bootstrap

Deterministic check to pair with the development gate: a test registers a task.assign hook whose reply is success false and asserts the task ledger is unchanged and the command fails with the hook's message

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-4`: the cron worker runs cron.tick hooks and replaces no atm task subcommand call
- `bk-1`: bk-1 is the two store capabilities only; replacing a subcommand call with a hook and acting on its reply is bk-3's router work
- `bk-5`: replacing the subcommand call with the hook and acting on its reply is bk-3's router; bk-5 adds the registry and cron commands and the doctor section

Fixtures:
- pass: The daemon router consults the active task.assign hook before admitting the assignment and parses the hook's JSON reply to decide whether the normal assign logic runs.
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

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: One JSON request schema with caller info for every hook

Scope: adds or changes the hook request type, its serialization, or what the hook runner writes to the hook's stdin

Deterministic check to pair with the development gate: one test serializes HookRequest for all six task hooks and cron.tick and asserts the same top-level keys: schema_version, hook, team, phase, caller, request, custom

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the HookRequest type and its one schema are bk-2; bk-1 stores registrations and schedules
- `bk-5`: the one request schema is bk-2's HookRequest; bk-5 sends no hook request

Fixtures:
- pass: HookRequest { schema_version, hook, team, phase, caller: CallerInfo { identity, team, host }, request, custom } is written to the hook's stdin for every task hook and for cron.tick.
- fail: Each task subcommand writes its own ad-hoc JSON to the hook, and the caller identity is passed only as an environment variable.

## ANC-004

Anchor: A success false reply must stop the subcommand before any of its logic runs, with the reply's message returned to the caller as the command error.

> if 'bd ready' false, atm-task-assign hook returns false and message to caller i.e. { success = "false" message="bead {bead-id} is not ready" }

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: success false reply refuses the command with the hook's message

Scope: adds or changes hook reply parsing, the refused path, or the error output of a hooked task subcommand

Deterministic check to pair with the development gate: a hook fixture prints {"success": false, "message": "bead x is not ready"}; the test asserts no task row is written and the error carries that message

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-1`: reply parsing and the refused path are bk-2 (types) and bk-3 (router); bk-1 runs nothing
- `bk-5`: the refused path is bk-3's router; bk-5 adds no task subcommand logic

Fixtures:
- pass: A reply of {"success": false, "message": "bead atm-bk-1 is not ready"} makes atm task assign exit non-zero with that message and writes nothing to the task ledger.
- fail: When the hook replies success false, atm logs a warning and continues with the normal assignment.

## ANC-005

Anchor: When a hook replies success true with no atm-action list, atm runs the subcommand's normal logic with the original request, giving the same result as when no hook is registered.

> or if 'bd ready' the hook either retruns { success = "true" } and atm proceeds w/ normal atm task assign logic

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: success true without actions = normal subcommand behavior

Scope: adds or changes hook reply parsing or the pass-through path of a hooked task subcommand

Deterministic check to pair with the development gate: a test with a hook replying {"success": true} asserts the resulting task row equals the row produced with no hook registered

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-1`: pass-through behaviour is bk-3's router path; bk-1 runs nothing
- `bk-5`: pass-through is bk-3's router path; bk-5 adds no task subcommand logic

Fixtures:
- pass: On {"success": true} the router falls through to the existing task-assign admission path with the caller's original request unchanged.
- fail: On {"success": true} the router reports success to the CLI without performing the assignment.

## ANC-006

Anchor: A success true reply with an atm-action array must make atm run each action in array order instead of the original subcommand's logic.

> OR the hook returns what to do { success = "true" atm-action[] = { action-1, action-2 } } where action-1 and action-2 would be atm json commands. i.e. action-1 could be exactly the same command that was input, action-2 could be 'atm send omega-prime@hermes <escelation-message>'.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: success true with atm-action runs the actions instead of the subcommand

Scope: adds or changes hook reply parsing or action dispatch

Deterministic check to pair with the development gate: a test with a reply carrying two actions asserts both ran in order and the original request was not applied

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-1`: action dispatch is bk-2's HookAction and bk-3's dispatcher; bk-1 runs nothing
- `bk-5`: action dispatch is bk-3's router; bk-5 adds no task subcommand logic

Fixtures:
- pass: On {"success": true, "atm-action": [...]} the router dispatches each action in array order through the internal command path and does not run the original assign logic.
- fail: The router runs the original assignment first and then runs the atm-action list as a follow-up.

## ANC-007

Anchor: Each atm-action entry is an atm command expressed as the same JSON body the daemon's HTTP API accepts for that command and is deserialized with that route's request type.

> 4. how I described #3 this would be true. an array of 'un-hooked' atm commands would be returned as json exactly the same as they would be passed over http.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: Actions are HTTP-API JSON bodies, no separate action dialect

Scope: adds or changes the atm-action entry type or how actions are deserialized and dispatched

Deterministic check to pair with the development gate: a test deserializes an atm-action entry's request field with the same serde type the HTTP route handler for that command uses

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the atm-action entry type and its deserialization are bk-1's HookAction and bk-3's dispatch_hook_reply; bk-4 calls the wrapper
- `bk-1`: the atm-action entry type is bk-2's HookAction; bk-1 defines no reply type
- `bk-5`: the action entry type is bk-2's HookAction; bk-5 parses no reply

Fixtures:
- pass: An atm-action entry is {"command": "task.assign", "as": "team-lead", "request": <the HTTP task-assign request body>} and is parsed with the HTTP route's request type.
- fail: Actions are shell command strings such as "atm send omega-prime hello" that the daemon splits and executes.

## ANC-008

Anchor: atm never consults a hook while executing an action returned by a hook; every atm-action entry runs through the un-hooked command path.

> the hook would not be called recursively, so the { success = true } (pass-through case) would simply do the same thing 'atm task assign ...' would do if there was no hook.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 2.

Summary: Hook-returned actions never trigger hooks

Scope: adds or changes action dispatch, the hook runner's entry points, or any path that runs a task subcommand on behalf of a hook

Deterministic check to pair with the development gate: a test whose task.assign hook returns an atm-action task.assign asserts the hook process was spawned exactly once

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-1`: hooks-disabled dispatch is bk-3's router flag; bk-1 has no dispatch path
- `bk-5`: hooks-disabled dispatch is bk-3's router flag; bk-5 dispatches no action

Fixtures:
- pass: Actions returned by a hook are dispatched with hooks disabled, so a task.assign action from the task.assign hook does not spawn the hook again.
- fail: Each atm-action entry is executed as a full CLI request, so a task.assign action consults the task.assign hook again.

## ANC-009

Anchor: An atm-action entry's as field is the identity the action runs as: atm dispatches the action with that caller identity, and an entry without as runs as the original caller.

> 6. yes

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal); point 6 of the proposal read: 'returned actions may run as the lead', 2026-10-18. Status: locked. Version 3.

Summary: An action runs as the identity named in its as field

Scope: adds or changes the atm-action entry type, action dispatch, or caller resolution for actions

Deterministic check to pair with the development gate: a test dispatches {command: task.assign, as: team-lead} from caller cipher and asserts the task row's assigner is team-lead

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the as field is bk-2's HookAction::envelope; bk-1 has no action type
- `bk-5`: the as field is bk-2's HookAction::envelope and bk-3's dispatch; bk-5 dispatches no action

Fixtures:
- pass: An action with "as": "team-lead" is dispatched with caller identity team-lead, so the resulting task assignment is recorded as assigned by team-lead.
- fail: The as field is ignored and every action runs as the identity that invoked the hooked command.

## ANC-010

Anchor: atm itself never performs a bead-tracker (bd) read or write; every bd call happens inside the hook process.

> 5. yes

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal); point 5 of the proposal read: 'bead writes stay inside the hook', 2026-10-18. Status: locked. Version 3.

Summary: atm never calls bd; bead logic lives in the hook

Scope: adds a dependency, subprocess call or storage access that touches beads, bd or .beads from any atm crate

Deterministic check to pair with the development gate: rg -n '"bd"|\.beads' crates/ matches nothing new on the sprint diff

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: bk-1 calls no process at all; the no-bd rule constrains bk-3's runner and bk-5's CLI
- `bk-2`: bk-2 calls no process at all; the no-bd rule constrains bk-3's runner and bk-5's CLI
- `bk-5`: bk-5's commands send envelopes and read TIMEOUT_MS; no bd call is possible from them, and the rule's code-level check is bk-3's

Fixtures:
- pass: The hook runner passes the bead id in the request and leaves every bd ready and bd close call to the hook module.
- fail: atm task assign runs bd ready itself before consulting the hook.

## ANC-011

Anchor: atm hook register must read an integer TIMEOUT_MS constant from the module's text, store it on the registration, and refuse a module that lacks one.

> 7. the timeout should probably be a required field of the hook python module i.e. const int timeout_ms = 1000.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal), 2026-10-18. Status: locked. Version 3.

Summary: TIMEOUT_MS is a required module constant read at registration

Scope: adds or changes atm hook register, module validation, or the timeout_ms column

Deterministic check to pair with the development gate: a test registers a module file without TIMEOUT_MS and asserts a typed refusal; with TIMEOUT_MS = 1000 the row's timeout_ms is 1000

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: TIMEOUT_MS reading and the register command are bk-5; bk-4 reads timeout_ms from the schedule row
- `bk-1`: reading TIMEOUT_MS at registration is bk-5's register command; bk-1 only stores timeout_ms with a positive CHECK
- `bk-2`: reading TIMEOUT_MS and refusing a module without it is bk-5's register command; bk-2 only carries timeout_ms in HookRegisterRequest
- `bk-3`: reading TIMEOUT_MS and refusing a module without it is bk-5's register command; bk-3's route validates the hook name only

Fixtures:
- pass: atm hook register parses TIMEOUT_MS = <int> from the module file, writes it to hook_registration.timeout_ms, and fails with HOOK.NO_TIMEOUT when the constant is absent.
- fail: The hook timeout is a daemon-wide setting in ~/.atm.toml and the module declares nothing.

## ANC-012

Anchor: The hook runner uses the registration's timeout_ms as the hook call's deadline: a hook still running at the deadline is killed and the command fails with HOOK.TIMEOUT.

> This allows the hook to KNOW when it's caller will timeout so it can set a shorted timeout to complete w/out error.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal) (point 7, second sentence), 2026-10-18. Status: locked. Version 2.

Summary: Runner enforces timeout_ms, kills the hook, fails with HOOK.TIMEOUT

Scope: adds or changes the hook runner's process spawn, wait, or deadline handling

Deterministic check to pair with the development gate: a paused-clock test with a sleeping fixture hook and timeout_ms 100 asserts the child is killed and the error code is HOOK.TIMEOUT

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: deadline enforcement and the kill on timeout are bk-3's DaemonHookRunner; bk-2 defines the HookTimeout code only
- `bk-1`: deadline enforcement and the kill on timeout are bk-3's DaemonHookRunner; bk-1 spawns nothing
- `bk-5`: deadline enforcement is bk-3's DaemonHookRunner; bk-5 spawns no hook

Fixtures:
- pass: HookRunner spawns the module with kill_on_drop, waits with tokio::time::timeout(timeout_ms), kills on expiry and returns AtmErrorCode::HookTimeout.
- fail: The runner waits for the hook indefinitely and the module's TIMEOUT_MS is only advisory.

## ANC-013

Anchor: Each of the six atm task subcommands (assign, start, close, move, list, events) must have its own hook name and consult its registered hook.

> assign,start,close are natural to override w/ hooks. we might as well hook the others

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 3.

Summary: All six atm task subcommands are hookable

Scope: adds or changes the hook name enum, the router's hook lookup, or any task subcommand's request path

Deterministic check to pair with the development gate: the hook name enum has task.assign, task.start, task.close, task.move, task.list, task.events and cron.tick; one test per subcommand asserts the hook is consulted

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: hooking the six subcommands is bk-3's router consult; bk-1 only names the six hooks in HookName::TASK_HOOKS
- `bk-5`: hooking the six subcommands is bk-3's router consult; bk-5 touches no task subcommand
- `bk-4`: bk-4 adds no task subcommand hook and does not change the hook name enum or the router's hook lookup; it only consumes cron.tick and cron.stop from bk-1 and bk-2 (not_applicable 0.76 in round 1, relevant-no 0.46 in round 3)

Fixtures:
- pass: HookName covers task.assign, task.start, task.close, task.move, task.list and task.events, and the router resolves the hook for each of the six before running it.
- fail: Only task.assign, task.start and task.close consult a hook; move, list and events are unhooked.

## ANC-014

Anchor: A task subcommand with no active hook for the caller's team and phase must run its existing logic unchanged and report no hook-related warning or error.

> since all hooks would be optional (defaulting to normal behavior in the case of missing), I don't see much risk.

Said by Rand, atm-core #1873 comment 2026-10-18T23:04Z (Rand, round 2), 2026-10-18. Status: locked. Version 4.

Summary: Missing hook = today's behavior, no error

Scope: adds or changes hook lookup or the task subcommand request paths

Deterministic check to pair with the development gate: the existing task CLI integration tests pass with no hook_registration rows and their output is unchanged; the router's request path makes no hook_registration store call (the in-memory snapshot is the only lookup; store call counter 0 after construction)

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: bk-2 defines the HookReply parse and action envelope only; running, refusing, passing through or dispatching in the subcommand path is bk-3's router work
- `bk-4`: no task subcommand request path is touched; the cron worker is a separate producer of hook runs
- `bk-1`: no task subcommand request path is touched by bk-1; the no-hook default is bk-3's snapshot path
- `bk-5`: the no-hook default is bk-3's snapshot path; bk-5 touches no task subcommand

Fixtures:
- pass: When no registration is active for the request in the router's in-memory snapshot, the router runs the existing admission path with no store read and the CLI output is unchanged from the pre-hook release.
- fail: atm task assign fails with HOOK.MISSING when no hook is registered for the team.

## ANC-015

Anchor: atm doctor, run for a team with an active hook registration, must invoke the module's doctor() through the hook runner and report the reply in a hooks section of the doctor report.

> the plug-in can run this check as part of 'doctor' it provides and IF a hook is installed, 'atm doctor' would call hook.doctor() when called from the team it is registered for.

Said by Rand, atm-core #1873 comment 2026-10-18T23:15Z (Rand, round 3: no sha header), 2026-10-18. Status: locked. Version 3.

Summary: atm doctor calls hook.doctor() for the registered team

Scope: adds or changes atm doctor, the doctor report sections, or doctor's use of the hook runner

Deterministic check to pair with the development gate: a test with an active registration for team T whose doctor() returns {"success": true, "message": "ok"} asserts run_doctor for T has a hooks section containing that reply

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: the atm doctor hooks section is bk-5; bk-2 only declares HookRunner::doctor
- `bk-1`: the atm doctor hooks section is bk-5; HookRunner::doctor is bk-2
- `bk-3`: the atm doctor hooks section is bk-5; bk-3 implements the runner's doctor method only (ADR-068 D3 AC)

Fixtures:
- pass: atm doctor adds a hooks section listing each active registration of the caller's team with the JSON reply of the module's doctor() call.
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
- `bk-5`: restart survival is bk-1's table and bk-4's active_all load at worker start; bk-5 composes the worker but asserts composition through run_now, not a restart
- `bk-3`: restart survival is bk-1's table and bk-4's active_all load; bk-3's cron routes call the scheduler trait only

Fixtures:
- pass: CronWorker::start reads every active row from CronScheduleStore at daemon boot and arms a timer for each, so no CLI call is needed after a restart.
- fail: Schedules exist only in the worker's memory; after a restart atm cron list is empty until atm cron start is run again.

## ANC-021

Anchor: A cron tick's hook reply goes through the same reply handler as a task hook's: an atm-action list is dispatched through the same un-hooked action path, and success false is recorded as the tick outcome.

> the cron design would be 'atm cron' and it would run a hook (just like the atm task hooks) at a predefined interval AND return a set of atm commands just like we described above.

Said by Rand, atm-core #1873 comment 2026-10-18T22:54Z (Rand, round 1, reply to the seven-point hook proposal) (cron note), 2026-10-18. Status: locked. Version 2.

Summary: Cron reply actions use the task-hook action path

Scope: adds or changes cron tick handling or hook reply handling

Deterministic check to pair with the development gate: a test with a cron.tick reply carrying one send action asserts a persisted message and a last_outcome of success

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: cron tick reply dispatch is bk-4; bk-2 defines the shared HookReply and HookAction types only
- `bk-1`: cron tick reply dispatch is bk-4 through bk-2's HookActionDispatcher; bk-1 defines no reply type
- `bk-5`: cron reply actions reach the task-hook dispatcher inside bk-4's tick; bk-5 only composes AtmCron

Fixtures:
- pass: The cron worker hands the tick reply to the same reply handler the task router uses, so returned actions are dispatched identically.
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
- `bk-4`: cron schedules have no owner-supplied custom (every cron request sends {}); the pass-through is bk-3's task path

Fixtures:
- pass: HookRequest.custom is a serde_json::Value taken from the request and written to the hook unchanged.
- fail: atm validates the custom object against its own schema and rejects unknown keys.

## REQ-BK-D1

Anchor: The hook runner must start a hook with exactly the environment ATM_HOOK=1, ATM_TEAM, ATM_PHASE, PATH and HOME in the registered cwd, passing no other variable from the daemon's environment.

> The hook runner starts a hook with exactly the environment ATM_HOOK=1, ATM_TEAM, ATM_PHASE, PATH and HOME in the registered cwd, and passes no other variable from the daemon's environment.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §2.1 D-1 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 3.

Summary: Fixed five-variable hook environment

Scope: adds or changes how the hook runner spawns the hook process

Deterministic check to pair with the development gate: a fixture hook that prints its environment shows exactly those five variables

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: the spawn environment is set by bk-3's DaemonHookRunner; bk-2 spawns no process
- `bk-4`: environment and cwd of the spawned hook are set by the shared DaemonHookRunner (bk-3); bk-4 reuses it
- `bk-1`: the spawn environment is set by bk-3's DaemonHookRunner; bk-1 spawns no process
- `bk-5`: the spawn environment is bk-3's DaemonHookRunner; bk-5 spawns no process

Fixtures:
- pass: HookRunner calls Command::env_clear() and sets ATM_HOOK, ATM_TEAM, ATM_PHASE, PATH and HOME, with current_dir set to the registration's cwd.
- fail: The hook inherits the daemon's full environment plus ATM_HOOK=1.

## REQ-BK-D3

Anchor: Hooks run inside the daemon process through one HookRunner shared by the task router and the cron worker; the atm CLI never spawns a hook process.

> Hooks run inside the daemon process through one HookRunner shared by the task router and the cron worker; the atm CLI never spawns a hook process.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §3 D-3 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 2.

Summary: One daemon-side HookRunner; the CLI spawns nothing

Scope: adds a hook-spawning code path in crates/atm or a second runner type

Deterministic check to pair with the development gate: rg -n 'process::Command' crates/atm/src matches no hook code; HookRunner is constructed once in atm-daemon-bootstrap and handed to the router and the cron worker

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the sealed HookRunner trait and the no-spawn CLI rule are bk-2 and bk-5; bk-1 is storage only

Fixtures:
- pass: HookRunner lives in atm-daemon-bootstrap and is passed by Arc to both StorageAndNudgeRouter and CronWorker; the CLI only sends the request over the socket.
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

Anchor: A hook that exits non-zero or replies with text that is not one of the three reply shapes fails the hooked command with HOOK.EXIT or HOOK.MALFORMED and runs no subcommand logic.

> A hook that exits non-zero or replies with text that is not one of the three reply shapes fails the hooked command with HOOK.EXIT or HOOK.MALFORMED and runs no subcommand logic.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §2.2 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 3.

Summary: Non-zero exit or malformed reply fails closed

Scope: adds or changes hook reply parsing or exit-status handling

Deterministic check to pair with the development gate: two tests: a fixture that exits 3 yields HOOK.EXIT; a fixture printing 'ok' yields HOOK.MALFORMED; neither writes a task row

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: the ATM_HOOK_* codes and fail-closed reply handling are bk-2 and bk-3; bk-1 raises only HookActive and constraint errors
- `bk-5`: fail-closed reply handling is bk-2's parse and bk-3's runner; bk-5 only prints the error codes it receives

Fixtures:
- pass: The runner maps a non-zero status to AtmErrorCode::HookExit and an unparsable stdout to AtmErrorCode::HookMalformed, and the router returns that error before admission.
- fail: A hook that prints nothing is treated as pass-through.

## REQ-BK-LOG

Anchor: Every hook call and every hook-returned action must emit one structured log entry carrying team, phase, hook, outcome and elapsed milliseconds (ADR-067).

> Every hook call and every hook-returned action emits one structured log entry carrying team, phase, hook, outcome and elapsed milliseconds (ADR-067).

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §3; ADR-067 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 3.

Summary: One ADR-067 log entry per hook call and per action

Scope: adds or changes the hook runner, action dispatch, or their tracing

Deterministic check to pair with the development gate: a tracing-capture test asserts one hook_call entry and one hook_action entry per action with the five fields

Excluded from (not judged; the cell passes as `excluded`):
- `bk-2`: structured log entries per hook run and action are emitted by bk-3's runner and router; bk-2 runs nothing
- `bk-1`: structured log entries per hook run and action are emitted by bk-3's runner and router and bk-4's worker; bk-1 runs nothing
- `bk-5`: the per-run and per-action log entries are bk-3's runner and router and bk-4's worker; bk-5 logs nothing new

Fixtures:
- pass: HookRunner emits tracing::info!(subsystem = "hook", team, phase, hook, outcome, elapsed_ms) once per call, and action dispatch emits one entry per action.
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

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §4; ADR-061 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 2.

Summary: Two new tables as one ADR-061 minor schema bump

Scope: adds or changes the storage schema or its migrations

Deterministic check to pair with the development gate: the schema version constant increments by one minor; a test opens a pre-bump database and asserts both tables exist after migration

Excluded from (not judged; the cell passes as `excluded`):
- `bk-4`: the storage schema change and the pre-bk DDL proof are bk-1; this sprint edits no storage crate
- `bk-2`: the storage schema change and the pre-bk DDL proof are bk-1; bk-2 edits no storage crate
- `bk-5`: the storage schema change is bk-1; bk-5 edits no storage crate
- `bk-3`: the storage schema change is bk-1; bk-3 edits no storage crate

Fixtures:
- pass: One migration adds hook_registration and cron_schedule and records the minor-bumped schema version; ADR-061's governed-interface table lists the bump.
- fail: Each sprint adds its own table with its own schema-version bump.

## REQ-BK-TESTS

Anchor: Hook and cron behavior is tested with a fixture hook module and a paused tokio clock; no test starts, targets or depends on a live daemon.

> Hook and cron behavior is tested with a fixture hook module and a paused tokio clock; no test starts, targets or depends on a live daemon.

Said by fenix (design.md, PR #1877), phase-bk design decision docs/plans/phase-bk/design.md §7 (fenix, PR #1877); not a Rand ruling, 2026-10-18. Status: locked. Version 2.

Summary: Tests use a fixture module and paused clock, never a live daemon

Scope: adds tests for hooks, cron, or doctor's hook section

Deterministic check to pair with the development gate: rg -n 'atm daemon|ATM_DAEMON|spawn.*daemon' on the new tests matches nothing; cron tests use #[tokio::test(start_paused = true)]

Excluded from (not judged; the cell passes as `excluded`):
- `bk-1`: bk-1's tests are store round trips on a temp database; no hook module runs and no clock is read
- `bk-2`: bk-2's tests are serde and parse unit tests; no hook module runs and no clock is read

Fixtures:
- pass: Tests under crates/atm-daemon-bootstrap/src/hook_runner.rs use a tests/fixtures/hook_module.py and #[tokio::test(start_paused = true)].
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
