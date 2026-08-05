---
name: closing-triage
version: 1.0.0
description: >
  Work a sprint branch's open triage findings down to zero, one finding at a
  time: query, fix cleanly, verify, test, commit/push, report, repeat.
depends_on:
  graph-orchestration: 1.x
---

# Closing Triage

Audience: the developer assigned to a sprint branch. Invoke with `/closing-triage`
while standing in that sprint's own worktree (the one you were assigned —
`../atm-core-worktrees/<your-branch>`, not an `integrate/*` worktree).

This skill does not decide who runs it or read any identity from the
environment; it operates purely on the current worktree's branch.

## What this skill does not do

- It does not write to the `.triage/*/findings/*.ttl` store. Finding closure
  (`triage:status`, `triage:Resolution`) is owned by QA/team-lead once they
  independently verify your fix — never by this skill or by you self-marking
  a finding fixed. Confirming the fix satisfies the finding (step d below) is
  your own sanity check before testing and committing, not a triage-store
  write.
- It does not dispatch QA reviewers. That happens separately, after you push.
- It does not decide phase/merge sequencing.

## Prerequisites

- You are in your assigned sprint worktree, on your assigned branch.
- `.claude/skills/closing-triage/scripts/query_open_findings.py` is reachable
  (present on this branch, or merged forward from wherever it landed).
- `rdflib` is installed (`pip install rdflib`) — required by the query script.
- There is exactly one reachable `integrate/phase-*` worktree for your phase,
  or you know its path to pass explicitly.

## The Loop

Repeat until step (a) returns zero findings for your branch. Do not stop
early because a fix "looks small enough to batch" — each finding gets its own
full pass through (a)–(g) so the query, the fix, the commit, and the report
stay traceable to exactly one finding.

### a) Query

From your sprint worktree, determine your current branch and run the query
against the integrate worktree for your phase:

```bash
BRANCH="$(git branch --show-current)"
python3 /path/to/.claude/skills/closing-triage/scripts/query_open_findings.py \
  --branch "$BRANCH" \
  --integration-root ../../atm-core-worktrees/integrate/phase-<name> \
  --phase <PHASE> \
  --json
```

Omit `--integration-root` only if you are literally standing in the integrate
worktree itself (you normally are not — you're in your sprint worktree). The
script refuses to run against anything that isn't an `integrate/*` branch, on
purpose: findings only live there, and a sprint worktree's own copy of the
triage store (if any) may be stale.

If the query returns zero findings, you are done. Stop here — do not invent
work.

### b) Take the most significant finding

Results are already ordered Blocking → Important → Minor (ties broken by
`found_at`). Take the first one. Do not skip ahead to an easier finding
further down the list, and do not batch multiple findings into one pass.

### c) Determine a clean, simplifying fix

Read the finding's full description and the file(s)/line(s) it points at.
Before writing any code:

- Confirm the defect the finding describes is still actually present at the
  cited location, in this branch's current state. If it doesn't reproduce
  (already fixed by other work, code path no longer exists, superseded),
  do not invent speculative work to "address" it — note why in your commit
  message instead and move to the next finding.
- Look for the fix that removes the underlying cause, not one that patches
  around it. If the finding describes duplicated logic, prefer consolidating
  to one owner over re-syncing two copies. If it describes a missing test,
  write the real test against the actual code path, not a shallow smoke
  check. Simpler and smaller is the goal — do not add abstractions, options,
  or generality the finding didn't ask for.

### d) Implement, then confirm the fix actually satisfies the finding

Make the change. Before moving on, re-read the finding's description against
your diff and answer honestly: does this fix the thing the finding actually
describes, or does it just make the symptom quieter? This is your own
verification pass — hold yourself to the same standard you'd want from an
independent reviewer. If the fix is partial, keep going until it isn't.

### e) Test

```bash
just test
```

Fix any failures your change introduced or exposed, then rerun. Repeat until
`just test` passes cleanly. Do not commit against a red test run.

### f) Commit, push, and reference the finding

Commit with a message that names the finding ID, so the fix is traceable back
to exactly the finding it closes:

```bash
git add -A
git commit -m "$(cat <<'EOF'
<short description of the fix>

Finding: <FINDING-ID>

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
git push origin "$BRANCH"
```

### g) Report completion

Send an ATM completion message to team-lead naming the fix and the commit:

```bash
atm send team-lead "Fixed <FINDING-ID>: <short description>. Branch <BRANCH> @ <commit-sha>."
```

Do not batch this across multiple findings — one message per finding fixed,
sent right after its own commit/push, so team-lead's view of progress stays
in sync with what's actually pushed.

Then return to step (a). The next query reflects your branch's real current
state — a finding you just fixed should no longer appear (it may still show
as open in the `.ttl` record itself until QA verifies and closes it; that's
expected and not a bug in this loop).

## Exit condition

The loop ends when step (a)'s query returns zero findings for your branch.
At that point every finding this tool can see against your branch has a
corresponding fix commit pushed. QA verification and finding closure in the
triage store happen next, outside this skill.
