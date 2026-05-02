---
name: restore-team-communications
version: 0.2.0
description: >
  Repair Claude teammate routing after same-session compaction or resume when
  atm-dev still exists on disk and the saved leadSessionId still matches the
  current SESSION_ID, but SendMessage or teammate reachability is broken.
---

# Restore Team Communications

Use this skill only when all of these are true:
- `ATM_IDENTITY=team-lead`
- `SESSION_ID` still matches `leadSessionId` in
  `~/.claude/teams/atm-dev/config.json`
- the team directory still exists on disk
- Claude teammate communication is broken or suspect after compaction or resume

Do not use this skill for fresh startup or after `clear`. If the current
`SESSION_ID` does not match `leadSessionId`, use `/team-lead` and follow the
full restore procedure instead.

## Step 0 — Prove Whether Repair Is Needed

First, try normal Claude-to-Claude communication before changing anything:

```text
SendMessage(to="<claude-teammate>", message="ping: verify atm-dev communications path")
```

If the message is delivered and acknowledged, stop. No repair is needed.

Do not read inbox files, session files, or tmux panes directly on this path.

## Step 1 — Back Up Current State

Back up before any destructive recovery:

```bash
atm teams backup atm-dev
BACKUP_PATH=$(ls -td ~/.claude/teams/.backups/atm-dev/*/ | head -1)
cp -r ~/.claude/tasks/agent-team-mail/ "$BACKUP_PATH/tasks-cc"
echo "CC task list backed up to $BACKUP_PATH/tasks-cc"
```

## Step 2 — Remove Broken Team Registration

Clear both live Claude routing state and the persisted team directory:

```text
TeamDelete
```

```bash
rm -rf ~/.claude/teams/atm-dev
```

If `TeamDelete` reports no active team name, proceed. That means live routing
was already absent.

## Step 3 — Recreate Team And Restore ATM State

Recreate the team:

```text
TeamCreate(team_name="atm-dev", description="ATM development team", agent_type="team-lead")
```

Then restore from the most recent backup:

```bash
atm teams restore atm-dev --from "$BACKUP_PATH"
```

If required members are missing after restore, add them before verification.

## Step 4 — Verify Both Communication Layers

Repair is not complete until all checks pass:

1. `SendMessage` to another Claude teammate.
2. `atm send` to a non-Claude model.
3. `atm send` to Codex and verify the nudge fires.

For Codex-directed ATM sends, the nudge must include a clear call to action, not
just a passive unread-mail announcement. Preferred structured nudge payload:

```text
<atm><action>read atm</action><action>ack <TASK-ID></action><action>execute assigned task</action><when idle="immediate" busy="after-current-task"/><console announce="concise" pause="false"/></atm>
```

Fallback plain-text wording:

```text
read atm for task <TASK-ID> and complete it before stopping
```

If the task is queued behind active work, use a queued-task nudge instead of an
interruptive one.

## Step 5 — Resume Work Quietly

If the repair succeeded:
- do not broadcast internal restore diagnostics over ATM
- send only the minimum teammate message needed to resume work
- return to normal project coordination

If the repair failed, stop and report the exact failed verification step.
