---
name: phase-orchestration
version: 0.1.0
description: Orchestrate multi-sprint phase execution as team-lead. Manages sprint waves, scrum-master lifecycle, PR merges, arch-ctm reviews, and integration branch strategy. This skill is for team-lead only, not for scrum-masters.
depends_on:
  scrum-master: 0.x
  rust-developer: 0.x
  rust-qa-agent: 0.x
  req-qa: 0.x
  rust-architect: 0.x
---

# Phase Orchestration

This skill defines how team-lead orchestrates a development phase consisting of
multiple sprints with dependency-aware parallelism.

Audience: team-lead only. Scrum-masters have their own process defined in
`.claude/agents/scrum-master.md`.

## Prerequisites

Before starting a phase:
1. The phase plan exists in `docs/project-plan.md` or a linked phase document.
2. The integration branch `integrate/phase-{N}` exists and is up to date with
   `develop`.
3. The Claude/ATM team is active.
4. `arch-ctm` is running and reachable via ATM CLI.

## Phase Execution Loop

### 1. Build the sprint dependency graph

Read the phase plan and identify:
- sprint dependencies
- parallel waves
- merge order within each wave

### 2. Execute sprints

For each sprint, respecting dependency order:

#### a. Spawn a fresh scrum-master

Each sprint gets a fresh scrum-master. Do not reuse scrum-masters across
sprints.

```json
{
  "subagent_type": "scrum-master",
  "name": "sm-{phase}-{sprint}",
  "team_name": "<team-name>",
  "model": "sonnet",
  "prompt": "<sprint prompt>"
}
```

Critical rules:
- `subagent_type` must be `scrum-master`
- `name` is required
- `team_name` is required
- scrum-master is a coordinator only
- scrum-master must not write code, run tests, or implement fixes itself

#### b. Sprint prompt template

The sprint prompt should include:
- phase and sprint id
- sprint title
- plan and requirements references
- worktree location
- branch name
- PR target
- reminder that scrum-master is a coordinator only

#### c. Monitor progress

- scrum-masters report completion or escalation to team-lead
- if a scrum-master reports sub-agent spawn failure, investigate and advise
- if a scrum-master escalates architecture risk, spawn `rust-architect`

### 3. Post-sprint: CI gate and merge

After each scrum-master reports completion:
1. verify QA passed
2. wait for CI green
3. merge PR to `integrate/phase-{N}` in dependency order
4. update the integration branch

### 4. Post-sprint: arch-ctm design review

After every sprint PR is merged to `integrate/phase-{N}`, request an `arch-ctm`
review via ATM CLI. Do not block the next eligible sprint unless arch-ctm
reports critical blocking findings.

### 5. Fix sprint if needed

If arch-ctm finds issues:
1. create a new worktree from `integrate/phase-{N}`
2. let arch-ctm or a fresh scrum-master execute the fixes
3. run `rust-qa-agent` and `req-qa` before merge

### 6. Wave transitions

Before starting the next wave:
1. all prerequisite sprints must be merged
2. integration branch must be current
3. critical arch-ctm findings must be addressed first
4. new scrum-masters start from the updated integration branch

### 7. Phase completion

After all sprints merge:
1. perform any phase-end version or release prep required by the plan
2. create PR `integrate/phase-{N} -> develop`
3. wait for CI green
4. merge after user approval
5. shut down remaining scrum-masters
6. do not clean up worktrees until user review

## Scrum-Master Lifecycle

- fresh per sprint
- named tmux teammate
- can spawn background sub-agents
- shut down after sprint completion
- never does dev work

## Team Lifecycle

- team persists across phases
- scrum-masters are ephemeral
- arch-ctm is persistent and communicates via ATM CLI

## ATM CLI Communication

Use ATM CLI for arch-ctm:

```bash
atm send arch-ctm "message"
atm read
atm inbox
```

Use tmux nudges when required by the local runtime setup.

## Anti-Patterns

- do not use `rust-developer` as the scrum-master subagent type
- do not tell scrum-masters to do dev work themselves
- do not do dev or QA work as team-lead
- do not skip post-merge arch-ctm reviews
- do not merge without QA pass and CI green
- do not reuse scrum-masters across sprints
