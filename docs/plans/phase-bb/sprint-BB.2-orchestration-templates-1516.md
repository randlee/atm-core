---
status: planned
branch: feature/bb2-orchestration-templates-1516
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/bb2-orchestration-templates-1516
---

# BB.2 — Orchestration templates at atm 1.5.16 (PARALLEL with BB.1)

| Field | Value |
| --- | --- |
| Schedule | **PARALLEL. Wave 1, starts with BB.1.** `parallel_safe` with BB.1 and BB.3 (plan §4) |
| Owner | cipher |
| Ruling | Rand 2026-09-12: "do we need to update the /graph-orchestration templates or prompts at all to handle the 'atm task' changes we are running now at atm v1.5.16? i.e. we want the tasks to be closed by dev and qa agents when the final report is sent. also when tasks are assigned, we should update fenced commands used to match v1.5.16." Plan P7 splits the 1.5.16 part (this sprint) from the post-BB.4 `atm task start` step (BB.7). |
| Depends on | nothing in `crates/`; the shipped host pair is prerelease 1.5.16 (`develop` 281e6f546) |
| Worktree | `feature/bb2-orchestration-templates-1516` off `integrate/phase-bb` |
| Governed interfaces | none |
| Requirements / ADRs edited | none; `docs/team-protocol.md` is BB.7's |

## What is wrong today (verified at `281e6f546`)

| File | Line | Today | 1.5.16 surface |
| --- | --- | --- | --- |
| `.claude/skills/graph-orchestration/SKILL.md` | 364–372 | `atm send arch-ctm --template … --var task_id="GO-$(date +%s)"` — `task_id` is only a template variable; no ATM task row is created | `atm send <agent> --task-id "$TASK_ID" --template … --vars …` (alias of `atm task assign`; `CLAUDE.md` 239–247) |
| `.claude/skills/codex-orchestration/SKILL.md` | 178 | same dispatch form without `--task-id` | same fix |
| `.claude/skills/graph-orchestration/dev-task.xml.j2` | 54 | "Send a completion message to team-lead via ATM including …" | `atm task close {{ task_id }} completed --stdin <<'EOF' … EOF` carrying the same content; the close is the completion message |
| `.claude/skills/graph-orchestration/dev-fix.xml.j2` | 70, 76 | "Send an ATM completion message to team-lead including …" | `atm task close {{ task_id }} completed …`; a fix the dev declines as not reproducible is still `completed` with the per-finding notes; a task the dev cannot do is `refused` with the reason |
| `.claude/skills/codex-orchestration/dev-template.xml.j2` | 61 (step g) | "send a brief completion report with the deliverable inventory" | `atm task close {{ task_id }} completed --stdin …` carrying the inventory; the push report of step e stays a plain `atm send` |
| `.claude/skills/codex-orchestration/fix-assignment.xml.j2` | 73 (step g) | "send a second report with PASS or FAIL" | `atm task close {{ task_id }} completed --stdin …` carrying PASS/FAIL and details |
| `.claude/skills/codex-orchestration/review-template.xml.j2` | 47 (step d) | "Send the structured findings report directly in the ATM message" | `atm task close {{ task_id }} completed --stdin …` carrying the findings report |
| `.claude/skills/codex-orchestration/qa-template.xml.j2` | 63 | `atm send team-lead --template ~/.atm/templates/quality-management-gh/findings-report.md.j2 --vars …` for every verdict | `atm task close {{ task_id }} completed --template ~/.atm/templates/quality-management-gh/<findings\|quality>-report.md.j2 --vars …` — the QA task is complete when the verdict is delivered, whatever the verdict |
| all six dispatch templates | first step (e.g. `review-template.xml.j2:44` "ACK this message immediately") | `atm ack` of the assignment | **unchanged in this sprint**: at 1.5.16 an assignment still requires an ack (forced, `send.rs:367`). BB.7 removes the ack step once BB.5 ships |

Memory `feedback_qa_verdict_needs_task_complete` recorded the symptom: a
plain send leaves the assigner's mirror task open and the reminder loop runs.

## Deliverables

- [ ] D1 — dispatch blocks in both `SKILL.md` files add `--task-id "$TASK_ID"`
  where `TASK_ID` is the same value passed as `--var task_id=…`; the
  sentence "that is the only sanctioned dispatch form" now names the
  `--task-id` form. `atm compose` preview line unchanged.
- [ ] D2 — every completion step named above becomes an `atm task close
  {{ task_id }} completed` fenced command with the existing required content
  as the report body (`--stdin` quoted heredoc for prose; `--template
  --vars` for the QA reports). `refused` is documented in `dev-fix.xml.j2`
  for the not-reproducible-and-nothing-else case only when the whole
  assignment cannot be done; per-finding "not reproduced" stays a note in a
  `completed` report.
- [ ] D3 — `qa-template.xml.j2` step l: both verdict templates go through
  `atm task close {{ task_id }} completed --template …`. Step k (install
  the daemon-readable templates) unchanged.
- [ ] D4 — host copies: no skill installs
  `~/.atm/templates/codex-orchestration/*.j2` today (only `qa-template.xml.j2`
  step k installs `quality-management-gh`). Add one install line to
  `.claude/skills/codex-orchestration/SKILL.md` beside the dispatch form:
  `mkdir -p ~/.atm/templates/codex-orchestration && cp .claude/skills/codex-orchestration/*.j2 ~/.atm/templates/codex-orchestration/`,
  run by team-lead after this PR merges. The two host-only variants
  (`dev-template-cipher.xml.j2`, `dev-template-solar.xml.j2`) are not in
  the repo: deliver the diff for them to team-lead in the completion
  report; do not edit `~/.atm` from this sprint.
- [ ] D5 — `.claude/skills/team-lead/SKILL.md` and
  `.claude/skills/codex-orchestration/SKILL.md`: the assigner's mirror-task
  rule (memory `project_task_state_mirror_task_on_assigner`): when the
  assignee closes with a report, the assigner's mirror closes with it; no
  separate `--task-complete` by team-lead is needed on 1.5.16 — verify in D6
  and document the verified behavior, not the memory.

## Tests

Live verification on the 1.5.16 host pair, recorded in the PR body with
message ids (no test code; this sprint has no Rust):

1. `atm task assign cipher --task-id BB2-PROBE --stdin` → `atm task list --all`
   shows `BB2-PROBE` assigned to cipher.
2. cipher runs `atm task close BB2-PROBE completed "probe"` → `atm task
   events BB2-PROBE` shows `completed`; `atm task list --all` shows no open
   `BB2-PROBE` on either cipher or the assigner.
3. `atm compose --template .claude/skills/graph-orchestration/dev-task.xml.j2
   --vars <vars>` renders the close command with the task id substituted.

Repo checks:

- `grep -rn "completion message" .claude/skills/graph-orchestration .claude/skills/codex-orchestration` returns nothing.
- `grep -rn "atm task close {{ task_id }}" .claude/skills/graph-orchestration .claude/skills/codex-orchestration` matches every dispatch template (six files: two graph, four codex).
- `.just/run_pytests.py` template-lint tests (if any cover these j2 files) pass.

## Acceptance criteria

1. Both dispatch blocks carry `--task-id`; every dispatch template closes its task with `atm task close`.
2. Live steps 1–3 recorded with ids.
3. D4 diff delivered to team-lead; D5 documents verified behavior.
4. `just lint spell` passes.

## Required validation

`just lint spell`, `just lint lines`; the three live steps.
