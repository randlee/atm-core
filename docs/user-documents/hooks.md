---
title: Hooks
audience: end-user
reviewed_for_release: 1.6.2
---

# Hooks

ATM supports best-effort post-send commands and daemon-managed task hooks.
Task hooks can handle, pass through, decline, or refuse a task command.

This document covers the operator-facing hook model only. It does not describe
developer implementation internals.

## Post-send hooks

### Hook Scope

Hook configuration belongs to the ATM-enabled repo surface for the active team
and identity.

Repo-local ATM hook configuration lives in `.atm.toml`.

The supported operator-facing hook/config surface is
`[[atm.post_send_hooks]]` for recipient-scoped external post-send commands.

Example:

```toml
[atm]
default_team = "atm-dev"

[[atm.post_send_hooks]]
recipient = "quality-mgr"
command = ["post-send-notify.sh"]
```

### Post-Send Hook Rules

`[[atm.post_send_hooks]]` is the supported full override path for post-send
behavior.

Important rules:

- `recipient` is one concrete member name or `*`
- multiple matching rules may run in config order
- path-like `command[0]` values resolve relative to the declaring `.atm.toml`
- bare executable names use normal `PATH` resolution
- post-send hooks are best-effort side effects and do not redefine whether ATM
  durably accepted a message
- if no matching external post-send hook rule exists, ATM falls back to the
  shipped built-in nudge path

The hook payload arrives in `ATM_POST_SEND` as ATM-owned JSON. That payload
includes the sender, recipient, team, `message_id`, description, task id,
ack-related flags, and other supported post-send fields.

## What a task hook is

A task hook is an optional input filter for `atm task assign`, `start`,
`close`, `move`, `list`, or `events`. Task history is not hooked.
Without an active registration for the caller's team, the command runs normally
and no hook process starts. The daemon runs the hook, never the CLI.

The four results are `handled`, `pass-through`, `not-applicable`, and
`refused`. A refusal happens before the command writes anything.

## The request

The daemon invokes `python3 <module> <hook-name>` with one JSON object on stdin:

```json
{
  "schema_version": "1.0.0",
  "hook": "task.close",
  "team": "atm-dev",
  "phase": "bj",
  "caller": {"identity": "trinity", "team": "atm-dev", "host": "mbp"},
  "request": {"task_id": "t-42", "outcome": "completed"},
  "custom": {"goal": "ship the release"}
}
```

`request` carries the command's bare HTTP body, not a tagged envelope; its
fields vary by command and may gain additive fields. `custom` is the JSON
object supplied at registration, unchanged. One module can serve many teams,
phases, agents, and goals: for example, a `jev_goal.py` module receives a
different `custom.goal` for each schedule, reads the repository from its
registered cwd, and identifies the cron caller from `request.created_by`.
ATM does not interpret the custom keys. The default object is `{}`; its
serialized size must not exceed 65,536 bytes.

## The reply

Print exactly one of these JSON objects:

```json
{"result": "pass-through"}
```

```json
{"result": "not-applicable"}
```

```json
{"result": "refused", "message": "the task is not ready"}
```

```json
{
  "result": "handled",
  "atm-action": [
    {
      "command": "task.start",
      "as": "trinity",
      "request": {"caller_team": "atm-dev", "caller_identity": "trinity", "task_id": "t-42"}
    }
  ]
}
```

`pass-through` runs the original command and stops the chain.
`not-applicable` tries the next registered phase; if all decline, the
original command runs. `refused` returns its message verbatim.
`handled` runs the actions instead of the original command. The older
`success` key is not a task-hook reply and produces `ATM_HOOK_MALFORMED`.

Actions may name any routed command except the hook and cron registry
commands. Their bodies are the corresponding bare HTTP request bodies.
Optional `as` selects another roster member of the same team; without it
the original caller is used. Empty action lists, a different team, or an
unknown `as` member are rejected before any action runs. Actions run in
order, without rollback: failure names how many actions already committed.
Returned actions run below the hook filter and never invoke task hooks.

The caller receives the last action response matching the original command,
or `hook ran N actions` when no action has that command. A handled dry run
validates every action but dispatches none.

## The module

Declare exactly one column-zero `TIMEOUT_MS = <digits>` or
`TIMEOUT_MS: int = <digits>` line. Spaces and a trailing `#` comment are
allowed; indentation, zero, a missing constant, or duplicate matching lines
are invalid. Task hooks are capped at 2,500 ms. Optional `CRON_TIMEOUT_MS`
uses the same grammar, defaults to `TIMEOUT_MS`, and is capped at 30,000 ms.
The CLI reads the file as text; the daemon enforces the caps. ATM does not
import the module in-process or store a module hash.

This 40-line example handles only `task.close` and `task.assign`; other
task commands return `not-applicable`. Save it as `task_hook.py`:

```python
import json
import sys

TIMEOUT_MS = 2500
CRON_TIMEOUT_MS = 30000

def hook(request):
    name = request["hook"]
    if name == "task.close":
        if request["custom"].get("block_close"):
            return {"result": "refused", "message": "close is blocked"}
        return {"result": "pass-through"}
    if name == "task.assign":
        return {"result": "pass-through"}
    return {"result": "not-applicable"}

def cron(args):
    return {"result": "not-applicable"}

def stop(args):
    return {"stop": False}

def doctor(request):
    return {"success": True, "message": "ok", "findings": [
        {"severity": "warning", "code": "EXAMPLE_REVIEW",
         "message": "Review the configured goal before release"}
    ]}

if __name__ == "__main__":
    request = json.load(sys.stdin)
    name = sys.argv[1]
    if name == "cron.tick":
        reply = cron(request["request"]["args"])
    elif name == "cron.stop":
        reply = stop(request["request"]["args"])
    elif name == "doctor":
        reply = doctor(request)
    else:
        reply = hook(request)
    print(json.dumps(reply))
```

A module returns actions rather than calling `atm` for its own hooked
command. A nested `atm` process is still bounded by the hook timeout.
On stdout emit only the one reply; use stderr for diagnostics.

## Errors you will see

| Code | CLI exit | Meaning |
| --- | --- | --- |
| `ATM_HOOK_MODULE_INVALID` | 3 | Invalid module, cwd, timeout, custom object, or registration |
| `ATM_CRON_EXPR_INVALID` | 3 | Invalid expression or stop time |
| `ATM_HOOK_REFUSED` | 3 | The hook's refusal message |
| `ATM_HOOK_EXIT` | 1 | Child spawn, Python exception, nonzero exit, or runner panic |
| `ATM_HOOK_TIMEOUT` | 1 | The module or remaining request budget expired |
| `ATM_HOOK_MALFORMED` | 1 | Invalid reply or action |

Malformed replies and runtime failures fail the command closed; they do not
fall back to its normal behavior. Check the named module and stderr tail,
correct the issue, and retry.

## Registering

Any roster member may register or disable hooks for the caller's team:

```sh
atm hook register task.close --module ./task_hook.py --phase bj --custom '{"goal":"ship"}'
atm hook register task.assign --module ./task_hook.py --phase bj --cwd .
atm hook list --phase bj --json
atm hook disable --phase bj task.close
atm hook disable --phase bj
```

Module and cwd must exist and are canonicalized; cwd defaults to the module's
parent. Registrations persist across restarts, and disabled rows remain
visible in `list`. Different phases may register the same task hook.
Active registrations run oldest first until one handles, passes through, or
refuses; each module should quickly decline tasks it does not own. The chain
shares the request deadline. Register only task hook names here, not
`cron.tick` or `cron.stop`.

## Hook mutation JSON contract

BK-9 freezes the machine-readable mutation result shapes. The persisted
registration object has exactly eleven fields and has no synthetic id. The
examples below are contract fixtures for BK-15; its composed CLI gate executes
these flags after the hook mutation options land.

<!-- fixture: hook.register-command -->
```bash
atm hook register task.close --module ./hooks.py --phase bk --json
```

<!-- fixture: hook.register-json -->
```json
{
  "action": "hook.register",
  "registration": {
    "team": "atm-dev",
    "phase": "bk",
    "hook": "task.close",
    "module_path": "/repo/hooks.py",
    "cwd": "/repo",
    "timeout_ms": 2500,
    "custom": {},
    "active": true,
    "registered_by": "trinity",
    "registered_at": "2026-10-23T00:00:00Z",
    "disabled_at": null
  }
}
```

Disable one hook, every hook in a phase, or a phase with no matches. The
disabled array contains the rows changed by the operation, and count always
equals its length. A no-match operation succeeds with an empty array and a
zero count.

<!-- fixture: hook.disable-one-command -->
```bash
atm hook disable --phase bk task.close --json
```

<!-- fixture: hook.disable-one -->
```json
{
  "action": "hook.disable",
  "disabled": [
    {
      "team": "atm-dev",
      "phase": "bk",
      "hook": "task.close",
      "module_path": "/repo/hooks.py",
      "cwd": "/repo",
      "timeout_ms": 2500,
      "custom": {},
      "active": false,
      "registered_by": "trinity",
      "registered_at": "2026-10-23T00:00:00Z",
      "disabled_at": "2026-10-23T01:00:00Z"
    }
  ],
  "count": 1
}
```

<!-- fixture: hook.disable-phase-command -->
```bash
atm hook disable --phase bk --json
```

<!-- fixture: hook.disable-phase -->
```json
{
  "action": "hook.disable",
  "disabled": [
    {
      "team": "atm-dev",
      "phase": "bk",
      "hook": "task.close",
      "module_path": "/repo/hooks.py",
      "cwd": "/repo",
      "timeout_ms": 2500,
      "custom": {},
      "active": false,
      "registered_by": "trinity",
      "registered_at": "2026-10-23T00:00:00Z",
      "disabled_at": "2026-10-23T01:00:00Z"
    },
    {
      "team": "atm-dev",
      "phase": "bj",
      "hook": "task.start",
      "module_path": "/repo/hooks.py",
      "cwd": "/repo",
      "timeout_ms": 2500,
      "custom": {},
      "active": false,
      "registered_by": "trinity",
      "registered_at": "2026-10-23T00:00:00Z",
      "disabled_at": "2026-10-23T01:00:00Z"
    }
  ],
  "count": 2
}
```

<!-- fixture: hook.disable-no-match-command -->
```bash
atm hook disable --phase missing --json
```

<!-- fixture: hook.disable-no-match -->
```json
{
  "action": "hook.disable",
  "disabled": [],
  "count": 0
}
```

These result examples do not change the existing human output, typed failures,
or the bare-array shape of atm hook list --json. See [Cron Schedules](./cron.md)
for the cron module contract and lifecycle examples.

## Cron

ATM cron runs the module on a persistent UTC schedule. For start, end, and
list commands, module replies, output mail, lifecycle notices, and recovery,
see the [Cron Schedules guide](./cron.md). The guide's command and reply
examples are linked to the shared fixture manifest under
[examples/cron](./examples/cron/manifest.json).

## Doctor

`atm doctor` calls each distinct module/cwd/team/phase combination among
the caller team's active hooks and schedules once. Its request is:

```json
{"schema_version":"1.0.0","hook":"doctor","team":"atm-dev","phase":"bj","custom":{"goal":"ship"}}
```

The example module returns one warning in the standard finding shape:

```json
{"success":true,"message":"ok","findings":[{"severity":"warning","code":"EXAMPLE_REVIEW","message":"Review the configured goal before release"}]}
```

`success` and `message` are required; findings are optional. Each finding
has severity `info`, `warning`, or `error`, a code, a message, and an
optional remediation. Entries appear under `Hooks:` with module and phase,
or `Hooks: none`; JSON always includes `hooks`, even when empty. Module
errors are reported in this section but never change doctor's overall summary.

## Platforms

On Unix the runner uses `python3` and a cleared environment containing only
`ATM_TEAM`, `ATM_PHASE`, `PATH`, and `HOME`, in the registered cwd.
On Windows it resolves `python` using `PATHEXT` and additionally needs
`SystemRoot` and `USERPROFILE`; `HOME` is optional. A Unix timeout kills
the process group; a Windows timeout kills only the child. Treat stdin JSON,
not other environment variables, as the module contract.

## Installed Docs vs Runtime State

Hook docs are installed under `share/doc/atm/`. Runtime state stays under
`~/.atm/`. Do not treat runtime state as the authoritative source for installed
long-form help content.

Example files live in [examples/hooks/](./examples/hooks/).

## Related Topics

- nudge payload shape and built-in overrides: [Nudge Templates](./nudge-templates.md)
- identity-sensitive behavior: [Identity And Team](./identity-and-team.md)
- recovery steps when a hook path fails: [Troubleshooting](./troubleshooting.md)

Return to the [ATM User Guide](./README.md).
