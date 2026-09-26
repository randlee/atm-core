# Role: dev-sanity (atm-bd-orchestration)

The dev-sanity role runs the sanity check of every closed dev or fix bead in
a phase run with atm-bd-orchestration. It is long-running; this role applies
to every task it receives until the lead switches it back.

A sanity check asks one question of a closed dev or fix bead: is the work
done? Nothing skipped, no obvious errors, lint passes. It is not QA. Leave
design, style and judgement to QA.

## Who Fills It

The skill names the role, never a member or an agent. The repository
decides both:

| Setting | Where | atm-core |
| --- | --- | --- |
| member | `roles.dev-sanity` in `.claude/project/orchestration.yaml` (fallback: `.claude/agents/registry.yaml`); print it with `.claude/skills/atm-beads/scripts/resolve-role dev-sanity` | `atm-sanity` |
| directive | that member's `[startup.<member>]` prompt in `.atm.toml` | `.claude/agents/dev-sanity-llm.md` |

The member name is unique to the team, because Herdr agent names are global
on the host. It is never a dev or fix agent, which would make sanity checks
wait behind their work. Switching how checks run (an LLM check, a jev check)
changes the directive in `.atm.toml`, not this skill.

## Tasks

Every task is a sanity check bead rendered from `dev-sanity-template.xml.j2`,
and the task id is the bead id. The template carries the task values and the
bead and task lifecycle; the member's agent prompt (its directive) says how
the check runs.

Sanity checks gate dependent dev work, so speed matters: the member starts
every open sanity check task at once, each with its own team of check
subagents, and closes them in whatever order their verdicts arrive. Nothing
waits on another check.

## Check Contract

One check is one closed bead at one pinned commit, split per deliverable:

- `scripts/sanity-split` reads the bead, parses the numbered list under
  `## Deliverables`, pins the commit, lists the changed files against the
  bead's `owned_paths`, starts the lint command in the background, and
  renders one assignment per deliverable from
  `templates/dev-sanity-assignment.json.j2`.
- The directive sends each assignment to its own check subagent as fenced
  JSON, all at once, and reads one fenced JSON result per deliverable back.
  The subagent owns that contract, in its `## Inputs` and `## Output Format`:
  [`.claude/agents/sc-sanity-llm.md`](../../../agents/sc-sanity-llm.md).
  Every check subagent (`sc-sanity-jev.md` too) keeps the same assignment
  and result, so a repository switches checks by switching the directive in
  `.atm.toml`.
- `scripts/sanity-merge` accepts exactly one result per deliverable at the
  pinned SHA, checks that the worktree is still at that SHA and clean,
  folds in the lint exit code and diagnostics, and writes the verdict and
  the report vars.

The check leaves nothing in the repository: `sanity-split` writes only the
lint log and the lint exit file under `--scratch`, the renderer's transient
input file is deleted once each assignment is rendered, and sc-compose keeps
its own log under `.sc-compose/`, which is gitignored.

There is no fallback. A bead whose `## Deliverables` is not a numbered list
cannot be split; the check is refused with `SANITY.PLAN_INVALID` and the
lead is told that planning failed for that bead. Every deliverable appears
in the report by number, done or with its findings, so closure is explicit.

## PR Gate

Before splitting a check, verify that the assignment names a reviewable pull
request. Refuse the task with `SANITY.PR_REQUIRED` when the assignment has no
PR, the PR is absent or closed, or the PR head is not the checked commit's
branch. Leave the bead open with the reason and close the task as `refused`
using `task-refused.md.j2`; send the refusal to the lead.

## Verdicts

| Verdict | Sanity check bead | Task |
| --- | --- | --- |
| PASS | `bd close` with reason `PASS at <commit>` | `completed`, `dev-sanity-complete.md.j2` |
| FAIL | stays open: `bd update --status open --append-notes` | `completed`, `dev-sanity-complete.md.j2` with the findings |
| cannot run | stays open, with a note | `refused`, `task-refused.md.j2` |

A FAIL never closes the bead. Closing it would release the dev beads that
depend on the checked sprint. The lead reopens the checked bead and assigns
the fix. When the fix closes, the same sanity check bead is ready again.
