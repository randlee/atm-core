---
name: team-lead
version: 0.4.0
description: >
  Session initialization for the team-lead identity. Confirms identity,
  verifies the ATM runtime, and keeps `.atm.toml` aliases, the ATM roster and
  the live Herdr agents in agreement. Only run when ATM_IDENTITY=team-lead.
---

# Team Lead Skill

Trigger: run at the start of every fresh session where `ATM_IDENTITY=team-lead`.
Do not use this skill for same-session compaction or resume unless Step 1
or Step 2 shows a problem, or a teammate stops receiving messages.

## Step 0 — Confirm Identity

```bash
echo "ATM_IDENTITY=$ATM_IDENTITY"
```

Stop if `ATM_IDENTITY` is not `team-lead`.

## Step 1 — Verify Runtime

```bash
which atm
atm --version
atm doctor --team "$ATM_TEAM"
```

Run `atm doctor` from the main checkout on `develop`. Doctor reads the
`.atm.toml` it finds above the current directory, so a worktree's copy can be
stale and a directory outside the repo skips the alias comparison silently.

If doctor reports the daemon unreachable, a binary mismatch or a wrong
`ATM_HOME`, stop and report it. Never restart or switch the daemon without
instruction, and never repair the roster on top of an unhealthy runtime. See
`atm help errors` for the reported codes.

## Step 2 — Verify The Roster

Three things must agree for every member: the alias declared in this
repository's `.atm.toml` (the file at the repo root, not `~/.atm.toml`), the
alias in the ATM roster, and the name of the member's live Herdr agent.

A member is declared in `.atm.toml` as one pane. `ATM_IDENTITY` is the member
name and `alias` is optional except for the three members named below:

```toml
[[rmux.windows.panes]]
name = "quality-mgr"
alias = "atm-quality"
env = { ATM_IDENTITY = "quality-mgr", ATM_TEAM = "atm-dev" }
```

- The daemon's store is the only roster. Read it with `atm members`, change it
  with `atm teams add-member` and `atm teams update-member`.
- For a Herdr-backed member the roster alias **is** the Herdr agent name ATM
  delivers to. With no alias, the member's own name is used.
- A Herdr agent name must be unique on the daemon's session, across every team
  on the host. Every project calls its members `team-lead`, `quality-mgr` and
  `publisher`, so those three **must** carry an alias (here `atm-lead`,
  `atm-quality`, `atm-publisher`). Another team's bare `publisher` on the same
  session is normal; ours must never answer to that name.
- `.atm.toml` is the declared truth. The roster and Herdr are brought into
  line with it, not the other way round.

One read-only command shows all three side by side and lists every problem:

```bash
python3 .claude/skills/team-lead/scripts/roster_check.py
```

It exits 0 when everything agrees. The raw sources, when you need them:

```bash
atm members --team "$ATM_TEAM" --json | jq -r '.members[] | "\(.name)\t\(.alias // "-")\t\(.backend)\t\(.state)"'
herdr agent list | jq -r '.result.agents[] | "\(.name)\t\(.pane_id)\t\(.cwd)"'
atm doctor --team "$ATM_TEAM" --json | jq '{alias_mismatches: (.alias_mismatches // []), drift_findings}'
```

`alias_mismatches` is `.atm.toml` against the roster. `drift_findings` is an
effective name shared with a member of another team.

Healthy means `roster_check.py` exits 0 and `atm doctor` has no team-member
finding. Then go to Step 4.

## Step 3 — Repair

Fix every team-member problem; do not work around one. Pick the row that
matches what `roster_check.py` or doctor printed.

| Problem | Fix |
|---|---|
| Roster alias missing or different from `.atm.toml` | `atm teams update-member "$ATM_TEAM" <member> --backend herdr --session default --alias <alias-from-.atm.toml>` |
| No live Herdr agent with the member's alias, and the agent is running under another name | find it with `herdr agent list`, then `herdr agent rename <pane_id> <alias-from-.atm.toml>` |
| No live Herdr agent and the agent is not running | report it; the member stays offline until Rand starts it. Do not point the roster at another agent |
| Member missing from the roster | `atm teams add-member "$ATM_TEAM" <member> --agent-type <type> --model <model> --home-dir <main checkout> --backend herdr --session default` plus `--alias <alias>` when `.atm.toml` declares one. Take `<type>` and `<model>` from Rand or from the member's previous roster row; do not guess |
| Herdr agent name not unique, or doctor `drift_findings` names a collision | give our member a unique alias in `.atm.toml` (next row), then apply it with the first two rows |
| `.atm.toml` is what is wrong: a required alias is missing, a roster member has no pane, or a pane has no roster member | change `.atm.toml` in a worktree created from `develop` with `/sc-git-worktree`, and open a PR to `develop`. Never edit it in the main checkout. Until the PR merges, report the mismatch as open |

Always pass `--alias` together with `--backend herdr` for a member that has
one, so the roster alias and the delivery target are written together. Use `update-member` on an existing member; never remove and
re-add. Other drifted fields take `--home-dir`, `--harness`, `--agent-type`
and `--model`.

After any repair, rerun Step 2 until it is clean, then prove delivery to each
repaired member:

```bash
atm send <member> "roster check: please ack" --team "$ATM_TEAM" --requires-ack
atm read --team "$ATM_TEAM"
```

ATM nudges the recipient on every send; there is no manual nudge. If the ack
never arrives, read that member's receiver line in `atm doctor` and report the
exact failed step rather than guessing.

`atm teams backup` and `atm teams restore --dry-run` exist for a corrupted
store. That is disaster recovery and needs Rand's instruction.

## Step 4 — Project Status

Read `docs/project-plan.md`, check open PRs with `atm gh pr list`, and output a
concise status: current phase, open PRs, each teammate's last known task, and
the next sprints ready to run. Stay silent in ATM unless a teammate has to act.

## Documentation

atm-core ships its own conceptual and command help — prefer it over
re-deriving procedural detail here:

```bash
atm help --list        # list conceptual topics and command help targets
atm help <topic>       # ATM-owned conceptual guidance (config, errors, hooks, identity, skills)
atm help <subcommand>  # clap-generated command help, e.g. `atm help teams`
atm <subcommand> --help
```

Tier-1/Tier-2 topics as of 1.3.1: `config`, `errors`, `hooks`, `identity`,
`skills`. Check `atm help --list` each session — the topic set and the
installed-docs index it reports can change between releases.

## Team-Lead Responsibilities

After initialization, use these repo-local skills to coordinate work:

| Skill | Trigger |
|-------|---------|
| `/phase-orchestration` | Orchestrate a multi-sprint phase with fresh scrum-masters |
| `/codex-orchestration` | Run phases where arch-ctm is sole dev, with pipelined QA via quality-mgr |
| `/plan-hardening` | Harden a phase plan and create any missing sprint docs before implementation starts or resumes |
| `/todo-triage` | Run the repo TODO scan during sprint-end or integration review and route TODOs into QA findings/Turtle triage instead of silent deferral |
| `/triaging-findings` | Correlate QA findings across branches before dispatching fixes to arch-ctm |
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

Assign work with `atm task assign`, never a plain `atm send`:

```bash
atm task assign <agent> --task-id <task-id> --template <template.j2> --vars <vars.json>
```

The template carries the workflow state, and the task assignment queues the
work and nudges the agent. A plain send opens no task, so the agent cannot
`atm task start` it and nothing re-nudges an agent that stops.

- Include task scope, worktree, relevant docs and acceptance criteria.
- The assignee runs `atm task start <task-id> "<one line>"` when `task_ready`
  arrives, reports at meaningful milestones, and closes with a commit or PR
  reference.
- **Every task must be closed.** Write the close into the assignment itself:
  the body ends with the instruction to run
  `atm task close <task-id> completed` with the commit or PR as the report
  when the work is done. The orchestration dispatch templates already end
  this way; a hand-written assignment must too. An agent's queue releases
  the next task only when the current one closes, so an open finished task
  blocks everything behind it.
- When work is reported complete, verify the task is closed with
  `atm task list --all`. If it is still open, close it yourself:
  `atm task close <task-id> completed "<what was delivered, commit or PR>"`.
  Do not spend a round trip asking the agent to close it. The assignee is
  told the assigner closed the task, so put the real result in the reason.

### Communication Rules

- No task start row means the work is not being done.
- Codex agents such as `arch-ctm` only see new ATM messages when they check
  mail after their current task completes.
- On atm 1.5.16, the assignee's `atm task close <task-id> completed` report
  closes the assigner's mirror task too. Team-lead reads that report and does
  not separately close the mirror with `--task-complete`.
- Use native `atm send` / `atm read` / `atm ack` for all teammate messaging.
  See `atm help identity` for how caller identity resolves for these
  commands.

## PR and CI Protocol

- Create the PR as soon as dev completes implementation and begins self-testing
  so CI runs in parallel with QA.
- Immediately after PR creation, start CI monitoring using the repo-local QA
  conventions from `.claude/skills/quality-management-gh/SKILL.md`.

## Cross-Host Cooperative Teams (m4/m5)

`team-lead@atm-dev.rand-m4` and `team-lead@atm-dev.rand-m5` are peers, each
running their own dev/`quality-mgr` under team `atm-dev`. They work
independently on separate phases of work, but integration and release
decisions (merging shared branches, publishing artifacts) require consensus
between both — neither side merges or releases unilaterally. Coordinate
scope with the peer before dispatching overlapping work, and address a
same-named peer identity with `atm send team-lead --host <hostname> "..."`.
