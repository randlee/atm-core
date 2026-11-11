# Sanity-only task-close hook

The hook and its deterministic unit tests ship in this skill and are installed
together. The paths below are relative to the installing repository.

Track this integration in one GitHub issue. Keep its description accurate by
removing erroneous or superseded requirements, and append comments recording
design decisions, changes, and test results. Do not create a new issue per gap.

Local rollout: `dev`, `fix`, and `qa` still use normal ATM close processing.
The daemon-mode sanity assignment instructions invoke only:

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

Disable with `atm hook disable --phase <x> task.close`. Registration is local daemon state, not a commit or deployment.

A dev or fix close (`atm-task-close.py`) reads `atm hook list --phase <x> --json` as the lead before it
dispatches sanity: an active `task.close` row for this module whose `sanity_agents` names the dev-sanity
member sets `sanity_close_mode: daemon` in the assignment; otherwise it is `legacy`. A failed read refuses
the close before anything is written.

Dev and fix replay keys include the current task assignment generation. A
reopened bead cannot close again at any prior-close commit recorded in its
completed task-event details or its bead metadata: push a
new rework commit before closing. An exact same-generation replay of an
already closed bead is idempotent. The originating generation is not authenticated.
If the assignment changes before a
journal write, the hook refuses with `CLOSE.STALE_ASSIGNMENT`. It reads back
the checked bead and sanity task after applying the close and reports
`CLOSE.POSTCONDITION` if the expected closed bead or assignment is absent.
The sanity iteration is the count of its task assignment events. The QA
assignment round comes from the QA bead's `metadata.round`, independently of
the sanity iteration. The sanity assignment's base is the PR base commit, the
layer's diff base; the selected sanity result records that base.
For daemon closes the selected result carries the assignment's full 40-hex base
unchanged. The hook checks that it is an ancestor of the checked commit in the
checked bead's worktree before writing. `SANITY_CLOSE.BASE` refuses a missing,
malformed or non-ancestor base. The assignment message is not independently
fetched to authenticate the submitted base; origin-bound provenance is tracked
in atm-bd#889.

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
data, including the dev relationship and blocking dependency. Dispatch does not
freeze a commit, so implementation tweaks remain possible while sanity is underway.
At close, the selected result must include `assignment_id` (the newest assignment
event message ID, or its sequence when no message ID exists) and `commit` (the
checked bead's current metadata commit). The hook refuses either value if it no
longer matches current task history or the checked bead. This prevents an old PASS
from closing a reopened assignment or a different implementation. The selected
result also supplies verdict, run ID and missing-item numbers/reasons (deliverable
zero denotes a verification-wide evidence deficiency).

All validation precedes mutations. Bead writes go to the bd-cache journal, not
to bd: PASS journals the sanity close; FAIL journals its findings in the checked bead notes, then the dev reopen (precondition:
the dev bead's assignee is the dev task's assignee) and the sanity reopen to
open/unassigned (precondition: its assignee is unchanged). Each entry's
idempotency key is derived from the close plan and the operation, so a retried
close never journals a write twice. The hook then starts a detached
`bd_cache.py drain` (stderr appended to `<db>.bd-cache.drain.log`); if it
cannot start, the bd-cache cron drains within one tick. PASS closes the sanity bead, then returns
unhooked ATM actions to close the dev task (unless already terminal) and sanity
task. FAIL waits within the remaining hook budget for its writes to apply before
returning an action that reassigns the checked task id with the phase dev or fix
template and variables rebuilt from the bead and repository configuration. If the
writes are pending, it refuses with `BEADS.WRITE_PENDING`; retry the same close
after the drain applies them. The bead notes carry the verbatim findings. The
sanity task then closes. Replay compares only the current assignment, ignoring
earlier cancelled assignments. Close evidence names the ledger top and PR base
separately and reports differing branches. PASS and FAIL are the only accepted verdicts. Missing deliverables
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
Hook refusal codes include `SANITY_CLOSE.INVALID`, `SANITY_CLOSE.BASE`,
`SANITY_CLOSE.BUSY`, `SANITY_CLOSE.COMMAND`, `SANITY_CLOSE.DATA`,
`SANITY_CLOSE.CONFIG`, `SANITY_CLOSE.LEGACY_VERDICT`, and `SANITY_CLOSE.INTERNAL`.

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
