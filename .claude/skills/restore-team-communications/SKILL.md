---
name: restore-team-communications
version: 0.3.0
description: >
  Repair native ATM teammate reachability when the SQLite-backed roster is
  healthy (per team-lead Step 1) but a specific teammate is unreachable —
  typically a stale Herdr agent alias after compaction, resume, or a pane restart.
---

# Restore Team Communications

Use this skill only when all of these are true:
- `ATM_IDENTITY=team-lead`
- `atm doctor --team "$ATM_TEAM"` and `atm members --team "$ATM_TEAM"` show a
  healthy runtime and a complete roster (per `team-lead` Step 1)
- one or more named teammates are unreachable or suspect despite the healthy
  roster

If the roster itself is missing members or `atm doctor` reports findings, use
`.claude/skills/team-lead/backup-and-restore-team.md` instead — this skill
does not touch roster membership beyond a single member's connection details.

## Step 0 — Prove Whether Repair Is Needed

First, try native ATM communication with the suspect teammate before changing
anything:

```bash
atm send <teammate> "ping: verify atm-dev communications path" --team "$ATM_TEAM" --requires-ack
```

If the message is delivered and acknowledged, stop. No repair is needed.

## Step 1 — Confirm Runtime And Roster Health

```bash
atm doctor --team "$ATM_TEAM"
atm members --team "$ATM_TEAM"
```

If either shows a problem outside a single member's connection details,
stop and use `.claude/skills/team-lead/backup-and-restore-team.md` instead.

## Step 2 — Repair The Stale Member Entry

The most common cause is a stale roster alias after the agent's Herdr pane was
restarted or renamed. The alias must equal the agent's name in Herdr.
Discover the live agent, then update the member in place — never remove and
re-add:

```bash
herdr agent list
atm teams update-member "$ATM_TEAM" <teammate> --backend herdr --session default --alias <herdr-agent-name>
```

`update-member` also accepts `--home-dir`, `--harness`, `--agent-type`, and
`--model` if one of those drifted instead of the alias.

## Step 3 — Verify Native ATM Communication

Repair is not complete until all checks pass:

1. `atm send --requires-ack` to the repaired teammate and receive its native
   ATM acknowledgement.
2. `atm send` to `quality-mgr` when that teammate is active, and verify ATM
   mailbox routing.
3. `atm send` to a Codex developer and verify the nudge fires. Nudges are
   built into ATM: every send nudges its recipient, and an assigned task
   queues and re-nudges an agent that stops working. There is no manual
   nudge. If it doesn't fire, check `atm doctor` for that member's receiver.

For Codex-directed ATM sends, the steer nudge must include a clear call to
action, not just a passive unread-mail announcement. Preferred structured
nudge payload:

```text
<atm><action>atm read --message-id {{message_id}}</action><action>atm task start <TASK-ID> "<one line>"</action><action>execute assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>
```

If the task is queued behind active work, use a nudge about the deferred task
instead of an interruptive one.

## Step 4 — Resume Work Quietly

If the repair succeeded:
- do not broadcast internal restore diagnostics over ATM
- send only the minimum teammate message needed to resume work
- return to normal project coordination

If the repair failed, stop and report the exact failed verification step.
