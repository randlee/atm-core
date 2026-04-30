---
name: team-lead
version: 0.1.0
description: >
  Session initialization for the team-lead identity. Confirms identity and
  detects whether a full team restore is needed. Only run when
  ATM_IDENTITY=team-lead.
---

# Team Lead Skill

Trigger: run at the start of every session where `ATM_IDENTITY=team-lead`.

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

- Match: the team is already initialized for this session, so no full ATM
  restore is needed. Before reading `docs/project-plan.md`, run
  `TeamCreate(team_name="atm-dev", description="ATM development team", agent_type="team-lead")`
  once to re-establish Claude Code's in-memory team routing if context
  compaction wiped it. This is a communications repair step, not a destructive
  restore.
- Mismatch or missing config: follow the full restore procedure in
  `.claude/skills/team-lead/backup-and-restore-team.md`.

## Team-Lead Responsibilities

After initialization, use these repo-local skills to coordinate work:

| Skill | Trigger |
|-------|---------|
| `/phase-orchestration` | Orchestrate a multi-sprint phase with fresh scrum-masters |
| `/codex-orchestration` | Run phases where arch-ctm is sole dev, with pipelined QA via quality-mgr |
| `/quality-management-gh` | Multi-pass QA on GitHub PRs; CI monitoring; findings/final quality reports |

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
