---
name: backup-and-restore-team
version: 0.4.0
description: Procedure for bootstrapping or repairing an ATM team's SQLite-backed roster. Referenced by the team-lead skill when Step 1's runtime/roster check finds a problem.
---
# Team Bootstrap And Recovery Procedure

Follow this procedure when Step 1 of the `team-lead` skill finds the runtime
unhealthy, or the roster missing/stale, for `$ATM_TEAM`.

atm-core 1.3.1+ keeps team membership in the daemon-owned SQLite store, not in
`~/.claude/teams/<team>/config.json`. There is no `TeamCreate` / `TeamDelete` /
`leadSessionId` model to reconcile — bootstrap and repair both go through
`atm teams add-member` / `atm teams update-member`.

Do not use this procedure to repair native ATM communication when the roster
is already healthy — use `/restore-team-communications` for that lighter path.

## Step 1 — Confirm Runtime Health

```bash
which atm
atm --version
echo "$ATM_DAEMON_BIN"
atm doctor --team "$ATM_TEAM"
```

If `atm doctor` reports findings (daemon unreachable, binary mismatch, wrong
`ATM_HOME`), resolve those first — a roster bootstrap on top of an unhealthy
daemon will not stick. See `atm help errors` for how to read the reported
error codes, and `atm help config` for the config surfaces `atm doctor`
checks.

## Step 2 — Check Current Roster

```bash
atm members --team "$ATM_TEAM"
```

Compare against the expected roster for `atm-dev`:

| Member | Agent type | Model | Home dir |
|--------|-----------|-------|----------|
| team-lead | team-lead | sonnet (or current session model) | `/Users/randlee/Documents/github/atm-core` |
| arch-ctm | codex | high | `/Users/randlee/Documents/github/atm-core` |
| quality-mgr | quality-mgr | sonnet | `/Users/randlee/Documents/github/atm-core` |

If a member's `tmuxPaneId` is stale (pane no longer exists or now hosts a
different agent), discover the correct pane before touching membership:

```bash
tmux list-panes -a -F '#{session_name}:#{window_index}.#{pane_index} #{pane_title} #{pane_current_command}'
```

## Step 3 — Add Or Update Members

Add any member missing from the roster:

```bash
atm teams add-member "$ATM_TEAM" team-lead --agent-type team-lead --model claude-sonnet-4-6 --home-dir /Users/randlee/Documents/github/atm-core --pane-id <pane>
atm teams add-member "$ATM_TEAM" {{TEAM_MEMBER}} --agent-type rust-arch --model codex-high --home-dir /Users/randlee/Documents/github/atm-core --pane-id <pane>
atm teams add-member "$ATM_TEAM" quality-mgr --agent-type quality-mgr --model claude-sonnet --home-dir /Users/randlee/Documents/github/atm-core --pane-id <pane>
```

Note the flag is `--agent-type`, not `--type`.

For a member that already exists but has a stale pane, model, or home dir,
use `update-member` instead of re-adding:

```bash
atm teams update-member "$ATM_TEAM" <member> --pane-id <correct-pane-id>
```

`update-member` also accepts `--home-dir`, `--harness`, `--agent-type`, and
`--model` for other drifted fields.

### Step 3b — Reapply Roster Aliases (required, not optional)

`add-member` and a bare `update-member` never carry an alias, so every
restore silently drops any alias a member had before — the roster falls
back to each member's bare name. If two teams on the same host both have a
`team-lead` or `quality-mgr` (common — every project's team-lead skill
names its lead `team-lead`), the bare names collide and roster writes for
*either* team can be rejected until the alias is restored.

For a herdr-backed member, the roster alias **is** the herdr agent name atm
targets for delivery — setting one without the other leaves the member
nudge-deaf. Do both together, per member, sourcing the intended alias from
this repo's `.atm.toml` (`[[rmux.windows.panes]].alias`):

```bash
atm teams update-member "$ATM_TEAM" <member> --alias <alias-from-.atm.toml>
herdr agent rename <member-or-current-pane-id> <alias-from-.atm.toml>
```

Do this for every member that has an `alias` entry in `.atm.toml`, not just
the ones that look broken — a stale bare name silently deadlocks unrelated
roster writes later (see the doctor output note in Step 4).

## Step 4 — Verify Roster And Runtime

```bash
atm members --team "$ATM_TEAM"
atm doctor --team "$ATM_TEAM"
atm doctor --team "$ATM_TEAM" --json
```

Confirm all three expected members are present, active, and the pane ids
resolve to real panes. This needs both doctor forms — they catch different
things and neither alone is sufficient:

- The plain-text output's `Alias mismatches:` section catches a member whose
  `.atm.toml` `config_alias` doesn't match its live `roster_alias` (`-` means
  Step 3b dropped or never applied it) — this can be true even with no
  active collision, and is **not** exposed in the JSON form.
- The JSON form's top-level `drift_findings` array catches an actual
  cross-team name collision (a member's effective name — alias if set, else
  bare name — is shared with another team's member) and, per member object,
  carries its live `alias` field to diff against `.atm.toml` directly.

Either non-empty means Step 3b was skipped or applied to the wrong member.
Also confirm `herdr agent list` shows each member's `name` equal to its
roster alias (not the bare role name) for every member `.atm.toml` declares
one for.

## Step 5 — Minimal Functional Check

ATM rejects self-addressed sends (`team-lead@$ATM_TEAM` may not send to
itself), so the round-trip must target another roster member:

```bash
atm send arch-ctm "restart check ($SESSION_ID)" --team "$ATM_TEAM" --requires-ack
atm read --team "$ATM_TEAM"
```

If this round-trip fails, capture and report the diagnostic bundle below
rather than guessing further:

```bash
which atm
atm --version
echo "$ATM_DAEMON_BIN"
atm doctor --team "$ATM_TEAM"
atm members --team "$ATM_TEAM"
```

## Step 6 — Read Project Context

1. Read `docs/project-plan.md`.
2. Recreate pending tasks if the task list is empty.
3. Output a concise project summary:
   - current phase and status
   - open PRs
   - active teammates and their last known task
   - next sprint or sprints ready to execute

## Step 7 — Notify Teammates

```bash
atm send arch-ctm "New session (session-id: $SESSION_ID). Team $ATM_TEAM verified. Please acknowledge and confirm status."
```

The recipient's `.atm.toml` `post_send_hooks` fires the nudge automatically on
send — no manual `tmux send-keys` nudge is needed. See `atm help hooks` for
how post-send hooks resolve and how to debug one that doesn't fire.

## Common Failure Modes

| Symptom | Cause | Fix |
|---------|-------|-----|
| `atm teams add-member` rejects `--type` | flag was renamed | use `--agent-type` |
| `atm doctor` reports daemon unreachable | daemon not running or wrong `ATM_DAEMON_BIN` | check `echo "$ATM_DAEMON_BIN"` matches the installed binary, restart daemon if needed |
| member present in `atm members` but sends never land | stale `--pane-id` | `atm teams update-member "$ATM_TEAM" <member> --pane-id <correct-pane-id>` |
| `atm send` fails with agent not found | member missing from roster | `atm teams add-member` per Step 3 |
| self-send or wrong identity routing | teammate launched with wrong `ATM_IDENTITY` | relaunch with the correct identity; see `atm help identity` |
| task list looks empty after a restart | Claude Code UI task panel stale state | create one real task through the task tool to refresh it |
| a member that used to nudge fine goes nudge-deaf right after a restore, or unrelated roster writes start failing with a name-collision error | Step 3's `add-member`/`update-member` dropped the member's alias — roster fell back to its bare name, which may collide with another team's same-named member on this host (e.g. two teams' `team-lead`) | Step 3b: reapply every alias from `.atm.toml` with `update-member --alias` and `herdr agent rename`, then confirm `atm doctor --json`'s `drift_findings` array is empty |

## Last Resort — Full Backup/Restore

Only needed for disaster recovery (corrupted SQLite store, wrong team
entirely). `atm teams backup` / `atm teams restore` operate on the real
daemon-owned store:

```bash
atm teams backup "$ATM_TEAM"
atm teams restore "$ATM_TEAM" --from <backup-path> --dry-run
atm teams restore "$ATM_TEAM" --from <backup-path>
```

Always run `--dry-run` first and review the plan before the real restore.
