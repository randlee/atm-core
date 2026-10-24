---
title: Cron Schedules
audience: end-user
reviewed_for_release: 1.6.2
---

# Cron Schedules

ATM cron runs a hook module on a persistent UTC schedule. Schedules belong to
the caller's team and a phase, survive daemon restarts, and remain visible
after they end. Use [Hooks](./hooks.md) for the module interface,
[Nudge Templates](./nudge-templates.md) for lifecycle notices,
[Errors](./troubleshooting.md) for recovery, and the [user guide](./README.md)
for the installed-document index.

## Start, inspect, and stop schedules

A schedule needs a unique name within its team and phase, a module, a cron
expression, and exactly one stop condition. Expressions use six fields with
seconds first, seven fields with an optional year, or one of the supported
aliases. Five-field expressions are refused. The daemon validates that the
expression has a future occurrence before it stores the schedule.

<!-- fixture: command.start-until-hook -->
```bash
atm cron start nightly --cron "0 */5 * * * *" --module ./cron.py --phase bk --cwd . --until-hook --arg status --arg --json --custom '{"goal":"ship"}'
```

This starts a schedule with a module stop check. Each --arg contributes one
value to the cron.tick request; the second value above is the literal --json.
Relative module and working-directory paths are resolved from the invocation
directory. The default working directory is the module's directory.

Use a future RFC 3339 instant when the schedule should stop at a fixed time.
ATM normalizes the stored instant to UTC:

<!-- fixture: command.start-until-utc -->
```bash
atm cron start reminder --cron "0 0 2 * * * 2030" --module ./cron.py --phase bk --until 2030-11-01T02:00:00+02:00
```

The six-field form starts with seconds; the seven-field form adds the year.
ATM supports @yearly, @monthly, @weekly, @daily, and @hourly. It rejects
five-field expressions. A module can declare CRON_TIMEOUT_MS for cron.tick
and cron.stop; when absent, ATM uses TIMEOUT_MS. The daemon caps the cron
timeout at 30,000 ms. A schedule receives one persisted jitter_s offset to
spread runs that share an expression.

These accepted expressions and the five-field rejection are parser fixtures:

<!-- fixture: expression.supported -->
```text
0 */5 * * * *
0 0 2 * * * 2030
@yearly
@monthly
@weekly
@daily
@hourly
```

<!-- fixture: expression.five-field-rejected -->
```text
* * * * *
```

The aliases can also be used in a start command:

<!-- fixture: command.start-alias -->
```bash
atm cron start hourly --cron @hourly --module ./cron.py --phase bk --until-hook
```

List schedules for the team, optionally filtered by phase. JSON output is a
bare array with the stored rows, including creator, generation, stop condition,
custom object, jitter_s, last_run_at, last_outcome, and ended_at:

<!-- fixture: command.list-json -->
```bash
atm cron list --phase bk --json
```

End one active schedule by name or end every active schedule in a phase:

<!-- fixture: command.end-one -->
```bash
atm cron end --phase bk nightly
```

<!-- fixture: command.end-phase -->
```bash
atm cron end --phase bk
```

Manual end records a stopped transition and retains the row. Ending an active
name and later starting it reuses the row with a new generation. An active
duplicate name is refused. Missed occurrences are not replayed, and one
schedule never runs more than one tick at a time.

## Module request and replies

ATM runs the module for cron.tick and, for --until-hook schedules, cron.stop.
The module receives one JSON request on stdin. cron.tick gets the schedule's
args and persisted creator under request; custom is the object supplied by
--custom, or {}. Values in args are not appended to the process argv. The
module prints one JSON reply to stdout; send diagnostics to stderr.

A pass-through reply can include normal output. A handled reply returns actions
instead of pass-through. An action with no `from` uses daemon-owned `cron@tool`;
a cron action may set an explicit ordinary `from` only when it equals the
schedule's persisted creator and passes BK-9 authorization.
The examples below are contract fixtures; BK-15 executes the same fixture IDs
through the production cron-only parser and controlled runner.

<!-- fixture: reply.handled-explicit-creator -->
```json
{
  "result": "handled",
  "atm-action": [
    {
      "command": "send",
      "from": "creator@atm-dev",
      "request": {
        "home_dir": "/repo",
        "current_dir": "/repo",
        "caller_identity": "cron",
        "caller_team": "atm-dev",
        "to": "team-lead@atm-dev",
        "message_source": {
          "Inline": "Nightly check complete"
        },
        "classification": {
          "category": null,
          "tags": [],
          "content_format": null
        },
        "summary_override": null,
        "requires_ack": false,
        "task_id": null,
        "dry_run": false
      }
    }
  ]
}
```

An explicit ordinary sender cannot name a different member or team. This
counterexample is marked as a BK-15 rejection fixture and is not a supported
reply:

<!-- fixture: rejection.foreign-from -->
```json
{
  "result": "handled",
  "atm-action": [
    {
      "command": "send",
      "from": "other@atm-dev",
      "request": {
        "to": "team-lead",
        "message": "status"
      }
    }
  ]
}
```

## Module output is ordinary mail

The optional cron-output array contains message or error items. Each item
becomes ordinary unread mail from cron@tool to the persisted creator, with
category cron and an output:message or output:error tag. Use atm list, atm
read, and atm search to inspect it. It has no implicit acknowledgement or
task. An error item is informational module mail; it does not by itself make a
successful cron run fail.

<!-- fixture: reply.custom-message-output -->
```json
{
  "result": "pass-through",
  "cron-output": [
    {
      "kind": "message",
      "body": "All checks passed"
    }
  ]
}
```

<!-- fixture: reply.custom-error-output -->
```json
{
  "result": "pass-through",
  "cron-output": [
    {
      "kind": "error",
      "body": "One source is unavailable"
    }
  ]
}
```

The resulting mail has this classification and destination:

<!-- fixture: output.custom-error -->
```text
From: cron@tool to creator@atm-dev
Category: cron
Tag: output:error
Body: One source is unavailable
```

Malformed output is refused as ATM_HOOK_MALFORMED. This example has an empty
body and is marked as a BK-15 rejection fixture:

<!-- fixture: rejection.malformed-output -->
```json
{
  "result": "pass-through",
  "cron-output": [
    {
      "kind": "message",
      "body": ""
    }
  ]
}
```

## Stop and lifecycle notices

With --until-hook, ATM calls cron.stop before each tick. Return stop false to
continue, or stop true with a reason to end the schedule. A true reply may
include final cron-output mail; the daemon then emits the separate stopped
nudge. Stop-check failure, malformed output, a non-zero exit, or timeout skips
that tick and leaves the schedule active; fix the module and let the next
scheduled check run.

<!-- fixture: reply.stop-false -->
```json
{
  "stop": false
}
```

<!-- fixture: reply.stop-true -->
```json
{
  "stop": true,
  "reason": "phase complete",
  "cron-output": [
    {
      "kind": "message",
      "body": "Phase complete"
    }
  ]
}
```

Daemon lifecycle notices use a separate nudge surface. They are not unread
messages and are not cron-output mail. The trusted default and override may
use only {{name}}, {{event}}, and {{detail_attribute}}. The event is stopped
or error; ATM creates the detail attribute as message="..." or error="...".
For example, an error notice and a stopped notice look like this:

<!-- fixture: output.lifecycle-error -->
```xml
<atm cron name="nightly" event="error" error="timeout: ATM_HOOK_TIMEOUT">
```

<!-- fixture: output.lifecycle-stopped -->
```xml
<atm cron name="nightly" event="stopped" message="phase complete">
```

Repeated identical failures are suppressed until success resets the failure
episode or a different failure occurs. A stop transition creates one durable
logical notice intent. Delivery may be accepted, suppressed, or unavailable;
a handoff with unknown status is retained for reconciliation. ATM guarantees
the durable logical intent, not exactly-once delivery by an external nudge
transport.

## Recovery

- For ATM_CRON_EXPR_INVALID, use six or seven fields with seconds first, or a
  supported @ alias; choose an expression with a future occurrence.
- For ATM_HOOK_MODULE_INVALID, check the module path, working directory,
  custom JSON object, and CRON_TIMEOUT_MS/TIMEOUT_MS cap.
- For ATM_HOOK_TIMEOUT, ATM_HOOK_EXIT, or ATM_HOOK_MALFORMED, inspect the
  module's stderr and fix its cron.tick or cron.stop reply. A failed stop check
  leaves the schedule active.
- Use atm cron list --json to inspect active state, last_run_at, and
  last_outcome. Use atm cron end to stop a schedule manually.
- Use atm teams set-nudge-template and disable-nudge-template with kind cron
  to manage lifecycle notices; disabling the template does not disable module
  output mail.

No direct storage edits are part of schedule recovery.

## Related topics

- [Hook registration and module contract](./hooks.md)
- [Nudge template commands and placeholders](./nudge-templates.md)
- [Error codes and troubleshooting](./troubleshooting.md)
- [Cron fixture manifest](./examples/cron/manifest.json)
