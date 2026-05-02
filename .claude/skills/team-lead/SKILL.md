---
name: team-lead
version: 0.1.0
description: >
  Session initialization for the team-lead identity. Confirms identity and
  detects whether a full team restore is needed. Only run when
  ATM_IDENTITY=team-lead.
---

# Team Lead Skill

Trigger: run at the start of every fresh session where `ATM_IDENTITY=team-lead`.
Do not use this skill for same-session compaction or resume unless the session id
has changed.

## Step 0 — Confirm Identity

```bash
echo "ATM_IDENTITY=$ATM_IDENTITY"
```

Stop if `ATM_IDENTITY` is not `team-lead`.

## Step 1 — Detect Whether Restore Is Needed

Get the current session id from the `SessionStart` hook output in context
(`SESSION_ID=<uuid>`). Compare it with `leadSessionId` in the team config:

```bash
python3 -c "import json; print(json.load(open('/Users/randlee/.claude/teams/atm-dev/config.json'))['leadSessionId'])"
```

- Match: the current session already matches the persisted team state. Proceed to
  reading `docs/project-plan.md` and outputting project status. Stay silent in
  ATM unless teammate action is required. If teammate communications are broken
  despite a match, stop and use `/restore-team-communications` instead of the
  full restore flow.
- Mismatch or missing config: this is the normal startup or `clear` case where
  the live `SESSION_ID` changed and the saved `leadSessionId` no longer
  matches. Follow the full restore procedure in
  `.claude/skills/team-lead/backup-and-restore-team.md`.

## Team-Lead Responsibilities

After initialization, use these repo-local skills to coordinate work:

| Skill | Trigger |
|-------|---------|
| `/phase-orchestration` | Orchestrate a multi-sprint phase with fresh scrum-masters |
| `/codex-orchestration` | Run phases where arch-ctm is sole dev, with pipelined QA via quality-mgr |
| `/quality-management-gh` | Multi-pass QA on GitHub PRs; CI monitoring; findings/final quality reports |
| `/restore-team-communications` | Repair same-session Claude teammate routing after compaction or resume without invoking full startup/clear restore |

Additional orchestration guides live in `.claude/skills/*/SKILL.md`.

### Phased Development — Mandatory

For any multi-sprint phased development, `/codex-orchestration` or
`/phase-orchestration` must be used as directed by the user.

After every session start or context compaction, if a phase is in progress:
1. identify which one skill governs the active phase
2. read only that skill
3. resume from the last documented state rather than memory alone

If unsure which orchestration skill applies, ask the user immediately.

## Task Assignment Protocol

When assigning work to a teammate:
1. create or update the task list entry first
2. include task scope, worktree, relevant docs, and acceptance criteria
3. require:
   - immediate ACK
   - intermediate status at meaningful milestones
   - completion notification with commit or PR reference

### Communication Rules

- No ACK means the work is not being done.
- Codex agents such as `arch-ctm` only see new ATM messages when they check
  mail after their current task completes.

## PR and CI Protocol

- Create the PR as soon as dev completes implementation and begins self-testing
  so CI runs in parallel with QA.
- Immediately after PR creation, start CI monitoring using the repo-local QA
  conventions from `.claude/skills/quality-management-gh/SKILL.md`.
