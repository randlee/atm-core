---
status: complete
branch: feature/bb7-docs
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb7-docs
---

# BB.7 — Documentation, ADR index, orchestration templates for `atm task start`

| Field | Value |
| --- | --- |
| Design | [`design.md`](../nudge-transition-templates/design.md) §3, §4.4, §4.7; plan §7 |
| Recommended | cipher / fast |
| Depends on | `must_follow` BB.6 (PR completion) and BB.2 (PR completion) |
| Worktree | `feature/bb7-docs` off `integrate/phase-bb` |
| Governed interfaces | none |
| Requirements / ADRs edited | `docs/adr/INDEX.md`; the per-sprint requirement/ADR edits are already landed by BB.1, BB.4, BB.5, BB.6 — this sprint verifies them (D5), it does not redo them |

## Deliverables

- [ ] D1 — `docs/team-protocol.md:24-41` "Task Commands": add `atm task start
  BA-123 "starting: reading the sprint doc"` to the closed set; state the
  lifecycle as the agent sees it: `task_queued` (informational, no action)
  → `task_ready` (read, start, execute) → `atm task start` → work →
  `atm task close`. `:54-80` "Message Classes": a task assignment is
  **informational** until `task_ready`; never `atm ack` an assignment;
  "task assignment" moves from the `requires_ack` examples to the
  informational examples. The "Daemon escalation messages" paragraph is
  unchanged.
- [ ] D2 — `CLAUDE.md:239-247` quick reference gains
  `| Start a task | atm task start <task-id> [message] |`; the alias
  paragraph keeps both existing aliases.
- [ ] D3 — `docs/agent-conventions.md`: new section `## Task lines (Phase BB)`
  after "AQ2 dual-channel delivery" (`:23`): the six task lines,
  one sentence each on what the reader does (`queued`: nothing; `ready` /
  `reminder`: read, start, execute; `started` / `complete` / `closed`:
  nothing, the mailbox row is the record).
- [ ] D4 — orchestration templates (BB.2 corrected them for 1.5.16; this
  adds the post-BB.4 surface): every dispatch template's first step
  becomes `atm task start {{ task_id }} "<one line: what you will do first>"`
  and the `atm ack` step is deleted (an assignment no longer requires an
  ack, BB.5). Files: `.claude/skills/graph-orchestration/{dev-task,dev-fix}.xml.j2`,
  `.claude/skills/codex-orchestration/{dev-template,fix-assignment,review-template,qa-template}.xml.j2`,
  both `SKILL.md` dispatch sections (the "ack -> work -> completion" line
  becomes "start -> work -> close"). The two host-only variants under
  `~/.atm/templates/codex-orchestration/` are reported to team-lead with
  the diff (BB.2 D4 rule).
- [ ] D5 — verify and, where a sprint missed one, land the plan §7 edits
  (`docs/requirements.md`, ADR-054, ADR-061, ADR-062); `docs/adr/INDEX.md`
  rows for the ADR-054 and ADR-062 Phase BB amendments and the ADR-061
  1.8.0 / 1.9.0 entries.
- [ ] D6 — `docs/plans/phase-bb/*.md` front-matter `status` fields to
  `complete` for merged sprints; `docs/project-plan.md` §60 "Phase BB"
  sprint rows to `merged (#PR)` for every merged sprint (the section itself
  landed with the plan, PR #1432); plan status and the §60 heading to
  "merged" once the phase PR merges (team-lead lands that last edit).

## Tests

- `grep -rn "atm ack" .claude/skills/graph-orchestration/*.j2 .claude/skills/codex-orchestration/*.j2` returns nothing.
- `grep -rn "atm task start {{ task_id }}" .claude/skills/graph-orchestration .claude/skills/codex-orchestration` matches all six templates.
- `atm compose --template .claude/skills/graph-orchestration/dev-task.xml.j2 --vars <vars>` renders the start and close commands with the id substituted.
- `just lint spell`, `just lint lines`.

## Acceptance criteria

1. D1–D4 landed; both greps hold; the compose render is in the PR body.
2. D5: every row of plan §7 is checked off in the PR body with the commit that landed it.
3. `just lint spell` passes.

## Required validation

`just lint spell`, `just lint lines`, the compose render.
