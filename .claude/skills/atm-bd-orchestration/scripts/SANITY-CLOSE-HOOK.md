# Sanity-only task-close hook

The hook and its deterministic unit tests ship in this skill and are installed
together. The paths below are relative to the installing repository.

Track this integration in one GitHub issue. Keep its description accurate by
removing erroneous or superseded requirements, and append comments recording
design decisions, changes, and test results. Do not create a new issue per gap.

Local rollout: `dev`, `fix`, and `qa` still use normal ATM close processing.
The existing `atm-task-close.py`, `sanity-close-vars`, and sanity merge scripts
are unchanged. The new sanity assignment instructions invoke only:

```
atm task close <sanity-bead> completed \
  --template <checkout>/.claude/skills/atm-bd-orchestration/templates/sanity-task-close.md.j2 \
  --vars <selected-vars.json>
```

Sanity produces selected-result JSON from its read-only review: PASS or FAIL,
run ID, and findings. It does not invoke a build-running split/merge helper.
No new shared-vars file and no QA reviewer configuration is needed for closing sanity. The daemon captures the template variables in the
request; the hook never reads an arbitrary result path from the caller.

Register from the primary checkout (the hook uses its registered cwd):

```
atm hook register task.close --phase <x> --cwd . \
  --module .claude/skills/atm-bd-orchestration/scripts/sanity_task_close_hook.py \
  --custom '{"task_prefix":"<prefix>-<x>-","sanity_agents":["<dev_sanity_member>"]}'
```

`sanity_agents` selects the enabled agent identities; other identities immediately
use default processing without querying bd. Bead labels additionally verify the
role for those agents.

Disable with `atm hook disable --phase bk task.close`. Registration is local daemon state, not a commit or deployment.

## Inputs and actions

Bead reads use bd-cache (`BD-CACHE.md`): the routing read is served from the
cache (one `bd list --id` on a miss), then one batched `bd list --id` of the
sanity and dev beads refreshes both before any decision, using the remaining
hook budget minus 0.5 s for task history and the reply. If that refresh fails,
the hook decides on the cached rows (pending journal entries applied), logs the
error to stderr and names it as `read_fallback` in any escalation. A bead in
neither the cache nor bd refuses and escalates.

The hook resolves stage from bead labels, dev ID from sanity metadata, the
blocking dependency from the bead graph, current assignees from ATM task events,
and phase lead from the phase configuration. `bd-validation` owns required bead
data, including the dev relationship and blocking dependency. There is no commit
pin requirement or extra dispatch checklist; implementation tweaks remain possible.
An observed commit may be retained as evidence provenance. The selected result
supplies verdict, run ID and missing-item numbers/reasons (deliverable zero denotes
a verification-wide evidence deficiency).

All validation precedes mutations. Bead writes go to the bd-cache journal, not
to bd: PASS journals the sanity close; FAIL journals the dev reopen (precondition:
the dev bead's assignee is the dev task's assignee) and the sanity reopen to
open/unassigned (precondition: its assignee is unchanged). Each entry's
idempotency key is derived from the close plan and the operation, so a retried
close never journals a write twice. The hook then starts a detached
`bd_cache.py drain` (stderr appended to `<db>.bd-cache.drain.log`); if it
cannot start, the bd-cache cron drains within one tick. PASS closes the sanity bead, then returns
unhooked ATM actions to close the dev task (unless already terminal) and sanity
task. FAIL reopens the dev bead, returns sanity to open/unassigned while keeping
its blocking edge, sends the developer a failure template, and closes sanity's
ATM task. PASS and FAIL are the only accepted verdicts. Missing deliverables
or missing/stale evidence are FAIL; sanity does not build. Legacy CANNOT_RUN
results are rejected with SANITY_CLOSE.LEGACY_VERDICT and escalated to the
phase lead without changing task/bead state. An assignment conflict, required
review-service outage, or verification-tool failure must name the exact
conflicting records or failed operation/error for the responsible owner.
Neither a legacy verdict nor an escalation grants permission to fabricate PASS/FAIL.
This initial handler neither dispatches QA nor links a stack.

## Failure ownership

Every escalation goes to the phase lead, including hook failures. The phase
lead routes remediation.

Invalid selected output belongs to sanity. Missing/mismatched planning data
belongs to the phase lead. Module/contract defects belong to the hook maintainer.
Validation errors return `ATM_HOOK_REFUSED` with code, field/detail, owner,
correlation ID and recovery. They do not close the task or mutate beads.

The hook assigns a tracked phase-lead escalation task,
`<original>.close-error-<error-id>`, using the escalation template. The same cause
reuses the same task instead of creating duplicate obligations. Failed assignment
is retained as pending and reported by `atm doctor`. The lead owns recovery until
the original close is verified; a plain notification is insufficient.
Hook failures, including unexpected exceptions, must report the failed operation,
error, known state (the journal entries already written), and recovery action
to the phase lead. Do not label an unknown state as an unmodified state.

Close plans and error escalations live in the local SQLite journal
`.atm-bd/sanity-close/journal.sqlite3`; journaled bead writes live in the bd-cache
file. ATM's normal structured log remains the hook-run/action log. No arbitrary
event-append API exists for ATM task history; task events record actual
assignment/close actions.

A slow or failing bd does NOT hold the task open: once the writes are journaled
the hook returns its ATM actions. The drain re-reads each bead before writing,
skips a write already achieved, and escalates to the phase lead (through the
bd-cache cron, once per entry) any write whose precondition no longer holds,
that `bd` refuses (exit 13), that names an absent bead, or that fails five
times. The lead sets the bead by hand and runs `bd_cache.py resolve <seq>`.
The daemon hook has no retry/sleep loop; its 2.5-second limit is unchanged.

Repair tasks assigned before the bd-cache cutover (`<sanity-id>.bd-repair-<key>`)
finish as before: the assignee runs `sanity-close-repair.py --journal <path> --key
<key>`, which uses the frozen pre-cutover hook `sanity_close_legacy.py`, and the
hook lets the repair task close only once its journal record says `repaired`;
`atm doctor` shows each unfinished one as `SANITY_CLOSE.REPAIR_PENDING`. No new
repair task is created. Delete the runner, `sanity_close_legacy.py`, the
`sanity-bead-repair.xml.j2` template and their tests once none remains open.

External ATM actions are sequential and are not a transaction. A notification
may repeat if delivery succeeded but acknowledgement was lost; never claim
exactly-once delivery. Undelivered error escalations appear in `atm doctor`;
escalated bead writes appear in `bd_cache.py status` and the cron's doctor.

Existing post-PASS stack input generation remains available after the close;
the stack handoff reads only the stack bead. Planning-driven QA dispatch and
other role migrations are separate work.

## Runtime and installation boundaries

The plugin can record and escalate exceptions while its handler is running. A
runtime failure before plugin execution, a killed/timed-out process, or failure
while ATM dispatches returned actions requires generic runtime logging/escalation.

Those runtime paths have not been established by the integration's unit tests;
they must be verified separately, including who receives the failed action and
how a tracked phase-lead obligation is created. Do not claim the integration
catches a process failure that prevents its code from running.

The upstream plugin installer is not present or verified in this test checkout.
Installing hook and tests together remains an integration packaging requirement,
not a claim that this checkout implements that installer.
