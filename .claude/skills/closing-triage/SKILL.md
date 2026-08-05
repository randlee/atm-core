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

## Why the loop needs a local task list

A finding's `triage:status` and any `triage:Resolution` record only change
once QA/team-lead verify and close it — never the moment you commit and push
a fix. Since this skill never writes to the triage store (see above), the
live query alone cannot tell that you already fixed something: it will keep
returning the same finding on every call until QA closes it upstream.

So you maintain your own **branch-local task list** — a plain JSON file at
`.git/closing-triage-tasklist.json` inside your worktree (`.git/` is never
tracked or pushed, so this stays purely local bookkeeping and never becomes
repo content). It is a list of objects:

```json
{
  "finding_id": "AJ6-ATM-QA-001-MEMBERS-CLI-MISSING",
  "severity": "blocking",
  "description": "...",
  "status": "queued",
  "commit_sha": null
}
```

`status` is one of `queued`, `implemented`, or `not-reproduced`. You are the
sole writer of this file. You work through `queued` items; the query is only
ever used to (re-)populate it, never to decide what's already done.

## The Loop

### a) Build or refresh the task list

Run the canonical query once:

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

Diff the result against your task list by `finding_id`: for every finding in
the query result that is **not already present** in your task list (under
any status), append it as a new `queued` entry (with its severity and
description recorded from the query). Never touch an entry that's already
recorded, whatever its status — a finding that still shows up in TTL because
QA hasn't closed it yet must not be re-queued. Re-sort the full set of
`queued` entries by severity (Blocking → Important → Minor, ties by the
query's own order) so step (b) stays correct across multiple refreshes.

If, after this diff, your task list has zero `queued` entries, you are done
— go to **Exit condition** below. Do not invent work.

### b) Take the most significant queued item

Take the first `queued` entry from your (now-sorted) task list. Do not skip
ahead to an easier item further down, and do not batch multiple items into
one pass.

### c) Determine a clean, simplifying fix

Read the finding's full description and the file(s)/line(s) it points at.
Before writing any code:

- Confirm the defect the finding describes is still actually present at the
  cited location, in this branch's current state. If it doesn't reproduce
  (already fixed by other work, code path no longer exists, superseded), do
  not invent speculative work to "address" it. There is no code change and
  therefore no commit for this item: `atm send team-lead` a short note
  naming the finding ID and why it didn't reproduce, set this task-list
  entry's `status` to `not-reproduced`, and go back to step (b) (skipping
  d–g, since there's nothing to implement, test, or push).
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

### f) Commit, push, and update the task list

Review what's actually changed before staging anything — your worktree may
already have unrelated in-progress or untracked files from other work. Stage
only the paths your fix touched, not the whole tree:

```bash
git status
git add <path1> <path2> ...   # exactly the files your fix touched -- never `git add -A`
git commit -m "$(cat <<'EOF'
<short description of the fix>

Finding: <FINDING-ID>
EOF
)"
git push origin "$BRANCH"
```

Then set this task-list entry's `status` to `implemented` and record the
commit SHA (`git rev-parse HEAD`) in its `commit_sha` field. Do not modify
any other entry, and do not touch the triage TTL.

### g) Report completion

Send an ATM completion message to team-lead naming the fix and the commit:

```bash
atm send team-lead "Fixed <FINDING-ID>: <short description>. Branch <BRANCH> @ <commit-sha>."
```

Do not batch this across multiple findings — one message per finding fixed,
sent right after its own commit/push, so team-lead's view of progress stays
in sync with what's actually pushed.

Return to step (b) if any `queued` entries remain in your task list.
Otherwise, return to step (a) to check whether new findings have appeared
against your branch since your last refresh.

## Exit condition

The loop ends when a refresh at step (a) adds zero new entries to your task
list *and* your task list has zero `queued` entries left. At that point, for
every finding the query could ever see against your branch: either it never
reproduced (recorded `not-reproduced`, reported), or you fixed it, tested it,
committed and pushed it, and recorded it `implemented` with its commit SHA.
None of that means QA has verified or closed anything yet — it only means
your side of the work is done. QA verification and finding closure in the
triage store happen next, outside this skill.
