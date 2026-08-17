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

## Scope boundary

This skill never writes to the `.triage/*/findings/*.ttl` store: finding
closure (`triage:status`, `triage:Resolution`) belongs to QA/team-lead after
they independently verify your fix. QA dispatch and phase/merge sequencing
happen elsewhere, after you push.

## Prerequisites

- You are in your assigned sprint worktree, on your assigned branch.
- `.claude/skills/closing-triage/scripts/query_open_findings.py` is reachable
  (present on this branch, or merged forward from wherever it landed).
- `rdflib` is installed (`pip install rdflib`) — required by the query script.
- Exactly one sibling `integrate/*` worktree exists — the query script
  auto-discovers it via `git worktree list`. Pass `--integration-root` only
  if discovery reports zero or multiple candidates.

## Why the loop needs a local task list

A finding's `triage:status` and any `triage:Resolution` record only change
once QA/team-lead verify and close it — never the moment you commit and push
a fix. Since this skill never writes to the triage store (see above), the
live query alone cannot tell that you already fixed something: it will keep
returning the same finding on every call until QA closes it upstream.

So you maintain your own **branch-local task list** — a plain JSON file in
your worktree's private git directory. In linked worktrees `.git` is a file,
not a directory, so always resolve the path with git itself:

```bash
TASKLIST="$(git rev-parse --git-path closing-triage-tasklist.json)"
```

This resolves to the per-worktree git directory (never tracked or pushed),
so the file stays purely local bookkeeping and never becomes repo content.
It is a list of objects:

```json
{
  "finding_id": "AJ6-ATM-QA-001-MEMBERS-CLI-MISSING",
  "severity": "blocking",
  "description": "...",
  "status": "queued",
  "commit_sha": null,
  "reason": null
}
```

`status` is one of `queued`, `implemented`, or `not-reproduced`. `reason` is
set only on `not-reproduced` entries (why the finding didn't reproduce); it
feeds the final team-lead summary. You are the sole writer of this file.
You work through `queued` items; the query is only ever used to
(re-)populate it, never to decide what's already done.

## The Loop

### a) Build or refresh the task list

Run the canonical query once:

```bash
BRANCH="$(git branch --show-current)"
python3 /path/to/.claude/skills/closing-triage/scripts/query_open_findings.py \
  --branch "$BRANCH" \
  --json
```

Run this from your sprint worktree — do not move to the integrate worktree.
The script auto-discovers the sibling `integrate/*` worktree via
`git worktree list` and queries its triage store, never a sprint worktree's
own (potentially stale) copy. It maps your branch to its declaring sprint
via the phase structure, so results are scoped to findings **found in your
own sprint** (`triage:foundIn`): findings from earlier sprints whose
defects also exist in your checkout are the upstream developer's work and
arrive fixed via merge — never fix them here. Pass `--integration-root`
only if worktree auto-discovery reports ambiguity, and `--phase` only if
your branch is declared in more than one phase structure.

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
  therefore no commit for this item: `atm send quality-mgr` a short note
  naming the finding ID and why it didn't reproduce (so QA can independently
  verify the finding no longer exists or was fixed elsewhere), set this
  task-list entry's `status` to `not-reproduced` and record that same reason
  in its `reason` field, and go back to step (b) (skipping d–g, since
  there's nothing to implement, test, or push).
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

Send an ATM completion message to quality-mgr naming the fix and the commit:

```bash
atm send quality-mgr "Fixed <FINDING-ID>: <short description>. Branch <BRANCH> @ <commit-sha>."
```

Do not batch this across multiple findings — one message per finding fixed,
sent right after its own commit/push, so quality-mgr's view of progress
stays in sync with what's actually pushed. Do **not** message team-lead per
finding; team-lead receives exactly one summary when your developer work is
complete (see Exit condition).

Return to step (b) if any `queued` entries remain in your task list.
Otherwise, return to step (a) to check whether new findings have appeared
against your branch since your last refresh.

## Exit condition

The loop ends when a refresh at step (a) adds zero new entries to your task
list *and* your task list has zero `queued` entries left. At that point, for
every finding the query could ever see against your branch: either it never
reproduced (recorded `not-reproduced`, reported to quality-mgr), or you
fixed it, tested it, committed and pushed it, and recorded it `implemented`
with its commit SHA.

You MUST then notify team-lead — exactly once, and only now — that your
developer work is complete, with the git commits containing the fixes.
(QA owns finding closure; this summary claims only that your side is done.)
This is the only message team-lead receives from this skill. Build the
payload directly from the task list at
`"$(git rev-parse --git-path closing-triage-tasklist.json)"` (every entry
appears in exactly one of the two arrays; fill the placeholders from the
task list, one object per finding):

````bash
atm send team-lead "$(cat <<'EOF'
Developer work complete for <BRANCH>.
```json
{
  "branch": "<BRANCH>",
  "implemented": [
    {"finding_id": "<FINDING-ID>", "commit_sha": "<sha>"}
  ],
  "not_reproduced": [
    {"finding_id": "<FINDING-ID>", "reason": "<why it didn't reproduce>"}
  ]
}
```
EOF
)"
````

None of this means QA has verified or closed anything yet — it only means
your side of the work is done. QA verification and finding closure in the
triage store happen next, outside this skill.
