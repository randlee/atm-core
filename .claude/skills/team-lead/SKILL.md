---
name: team-lead
version: 0.3.0
description: >
  Session initialization for the team-lead identity. Confirms identity,
  verifies the ATM runtime and SQLite-backed roster, and bootstraps or
  repairs the team when needed. Only run when ATM_IDENTITY=team-lead.
---

# Team Lead Skill

Trigger: run at the start of every fresh session where `ATM_IDENTITY=team-lead`.
Do not use this skill for same-session compaction or resume unless the runtime
checks in Step 1 show a problem.

## Step 0 — Confirm Identity

```bash
echo "ATM_IDENTITY=$ATM_IDENTITY"
```

Stop if `ATM_IDENTITY` is not `team-lead`.

## Step 1 — Verify Runtime And Roster

atm-core 1.3.1+ has no Claude-Code-native team registry. The daemon's SQLite
store is the sole source of truth for team membership; there is no
`config.json` or `leadSessionId` to compare against. Verify runtime and
roster directly:

```bash
which atm
atm --version
echo "$ATM_DAEMON_BIN"
atm doctor --team "$ATM_TEAM"
atm members --team "$ATM_TEAM"
```

Branch on the result:

- **Runtime healthy, roster complete** (team-lead, arch-ctm, quality-mgr all
  present and active): proceed to reading `docs/project-plan.md` and
  outputting project status. Stay silent in ATM unless teammate action is
  required.
- **Runtime healthy, roster missing or stale members**: follow
  `.claude/skills/team-lead/backup-and-restore-team.md` to bootstrap or repair
  the roster.
- **Runtime unhealthy** (`atm doctor` reports findings, daemon unreachable,
  binary path wrong): follow the diagnostics-capture step in
  `.claude/skills/team-lead/backup-and-restore-team.md` before touching the
  roster.
- **Roster looks fine but native ATM communication is broken** (e.g. a
  teammate doesn't ack a `--requires-ack` send): use
  `/restore-team-communications` instead of re-running the bootstrap.

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
| `/restore-team-communications` | Repair native ATM teammate reachability when the roster is healthy but communication is broken |

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
