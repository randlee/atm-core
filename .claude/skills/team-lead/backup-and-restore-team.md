# Team Backup And Restore Procedure

Follow this procedure when Step 1 of the `team-lead` skill detects a session id
mismatch and a full team restore is required.

## Step 2 — Backup Current State

Always back up before modifying the team:

```bash
atm teams backup atm-dev
```

Also back up the Claude Code project task list separately:

```bash
BACKUP_PATH=$(ls -td ~/.claude/teams/.backups/atm-dev/*/ | head -1)
cp -r ~/.claude/tasks/agent-team-mail/ "$BACKUP_PATH/tasks-cc"
echo "CC task list backed up to $BACKUP_PATH/tasks-cc"
```

Note: `atm teams backup` captures ATM team tasks under `~/.claude/tasks/atm-dev/`
when present, but not the repo-local Claude Code task bucket
`~/.claude/tasks/agent-team-mail/`.

## Step 3 — Clear Stale Team State

```text
TeamDelete
```

Then remove the stale team directory so the next create uses the correct name:

```bash
rm -rf ~/.claude/teams/atm-dev
```

If `TeamDelete` already removed the directory, the `rm -rf` is harmless.

## Step 4 — Create Team

```text
TeamCreate(team_name="atm-dev", description="ATM development team", agent_type="team-lead")
```

Verify that the returned team name is exactly `atm-dev`. If it is not, stop.

## Step 5 — Restore Team Members And Inboxes

```bash
atm teams restore atm-dev --from ~/.claude/teams/.backups/atm-dev/<timestamp>
```

Verify members:

```bash
atm members
```

If unexpected ghost members exist, trim the config manually:

```bash
python3 -c "
import json
path = '/Users/randlee/.claude/teams/atm-dev/config.json'
with open(path) as f:
    cfg = json.load(f)
keep = ['team-lead', 'arch-ctm', 'quality-mgr']
cfg['members'] = [m for m in cfg['members'] if m['name'] in keep]
with open(path, 'w') as f:
    json.dump(cfg, f, indent=2)
print('Members:', [m['name'] for m in cfg['members']])
"
```

Adjust the `keep` list if additional named teammates are intentionally active.

## Step 6 — Restore Claude Code Task List

```bash
BACKUP_PATH=$(ls -td ~/.claude/teams/.backups/atm-dev/*/ | head -1)
if [ -d "$BACKUP_PATH/tasks-cc" ]; then
  mkdir -p ~/.claude/tasks/agent-team-mail
  cp "$BACKUP_PATH/tasks-cc/"*.json ~/.claude/tasks/agent-team-mail/ 2>/dev/null || true
  MAX_ID=$(ls ~/.claude/tasks/agent-team-mail/*.json 2>/dev/null \
    | xargs -I{} basename {} .json \
    | sort -n | tail -1)
  [ -n "$MAX_ID" ] && echo -n "$MAX_ID" > ~/.claude/tasks/agent-team-mail/.highwatermark
  echo "Task list restored. Highwatermark: $MAX_ID"
else
  echo "No tasks-cc/ in backup — task list not restored."
fi
```

The Claude Code UI task panel may not show restored tasks until one task is
created through the task tool.

## Step 7 — Verify Team Health

```bash
atm members
atm inbox
atm gh pr list
```

## Step 8 — Read Project Context

1. Read `docs/project-plan.md`.
2. Recreate pending tasks if the task list is empty.
3. Output a concise project summary:
   - current phase and status
   - open PRs
   - active teammates and their last known task
   - next sprint or sprints ready to execute

## Step 9 — Notify Teammates

```bash
atm send arch-ctm "New session (session-id: <SESSION_ID>). Team atm-dev restored. Please acknowledge and confirm status."
```

If no response arrives within about 60 seconds, nudge via tmux:

```bash
tmux list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_title}'
tmux send-keys -t <pane-id> "You have unread ATM messages. Run: atm read --team atm-dev" Enter
```

## Common Failure Modes

| Symptom | Cause | Fix |
|---------|-------|-----|
| `TeamCreate` returns random name | `~/.claude/teams/atm-dev` still exists | remove the directory and retry |
| `TeamDelete` says no team name found | fresh session with no active team context | expected, proceed |
| task list looks empty after restore | highwatermark mismatch or UI stale state | set `.highwatermark`, then create one real task |
| `atm send` fails with agent not found | member missing after restore | add the member back to the team |
| self-send or wrong identity routing | teammate launched with wrong `ATM_IDENTITY` | relaunch with the correct identity |
