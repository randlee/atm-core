---
name: gh-stack-view
version: 1.0.0
description: One-call coherence and mergeability table for a gh stack (head/base SHAs, origin vs local, needsRebase, mergeable, mergeStateStatus, CI). Use for any stacked-PR status question, before/after a rebase or merge, or `/gh-stack-view`. Never check stack layers one branch at a time.
---

# gh-stack-view

Read-only stack status. One command replaces the "one `gh pr view` per branch"
habit, which never shows the two things that actually break stacks:

1. **Base coherence** - is each layer's base SHA the head SHA of the layer
   below (and the bottom layer's base the trunk head)? Only
   `gh stack view --json` exposes `head` and `base`; `gh pr view` does not.
2. **Rebase / mergeability** - `needsRebase` from gh stack plus GitHub's
   `mergeable` and `mergeStateStatus` (`CONFLICTING`, `BEHIND`, `DIRTY`,
   `BLOCKED`, `CLEAN`). `gh pr list` and `gh pr checks` do not return
   `needsRebase`, and `gh pr checks` output must never be text-parsed.

The script also compares every local head against `origin/<branch>` and the
PR's `headRefOid`, so stale local tracking (someone else rebased the stack) is
caught before anyone runs `gh stack sync` on top of it.

## Usage

```
/gh-stack-view [--phase aw | --trunk <branch>] [--all] [--no-fetch] [--no-pr] [--json]
```

Run from anywhere in the repo, normally the main checkout on `develop`. The
script discovers every stack by running `gh stack view --json` in each
worktree from `git worktree list` (concurrently), keeps the longest view of
each stack (a lower-layer worktree only sees the layers linked from it), and
joins them into one report. The default view is strict: only stacks whose
trunk is the current phase (highest `integrate/phase-*` seen) or `develop`,
and only stacks that still have an open layer. Merged, closed and other-phase
stacks are counted on a `hidden:` line; `--phase`/`--trunk` selects another
trunk, `--all` shows everything.

```bash
python3 .claude/skills/gh-stack-view/scripts/gh_stack_view.py --phase aw
```

Exit codes:

| Code | Meaning |
|------|---------|
| 0 | every shown stack coherent |
| 1 | problems listed under a VERDICT |
| 2 | nothing to show or the environment failed; stderr says which (see Errors) |

## Errors

Every failure is one `gh-stack-view: ...` line on stderr with the next action,
never a traceback. Exit 2 covers all of these, so read the line:

- `git`/`gh` not on PATH, gh-stack extension missing (`gh extension install github/gh-stack`), not inside a git repository.
- `gh repo view` / `gh api graphql` failure: run `gh auth status`; `--no-pr` gives a local-only view meanwhile.
- GraphQL errors, null data or non-JSON output: same, with the PR numbers named.
- `gh stack view --json` returning an unexpected shape: upgrade gh-stack.
- No open stack for the current phase or develop: the line reports how many merged/closed/other-phase stacks were hidden; use `--phase`, `--trunk` or `--all`.

Non-fatal: a failed `git fetch origin` is warned once and the rebase column
shows ❓ instead of comparing against stale refs. Pruned or unreadable
worktrees are skipped silently (`git worktree prune` cleans them up).

## Output

The script renders everything. **Paste its output verbatim and unfenced**
(no ``` around it) so the markdown table renders in the terminal. The agent
makes no rendering decisions: no reformatting, no re-summarising, no
substituting its own per-branch lookups. Example (as it should appear):

stack: fix/aw-pool-read-migration -> integrate/phase-aw @ 0e640b20a

| L | PR | rebase | merge | CI |
|---|---|---|---|---|
| 1/2 | #1242 | ✅ | 🚧 | 🌀 |
| 2/2 | #1244 | ✅ | 🚧 | 🌀 |

VERDICT: ✅ COHERENT - every base == parent head, every head pushed and on its PR

| Column | Source | Icons |
|--------|--------|-------|
| L | layer / stack depth, bottom first | |
| rebase | `gh stack view --json` `head`/`base`/`needsRebase`, `origin/<branch>` after one fetch, PR `headRefOid` | ✅ not needed (base==parent head and local==origin==PR) · ⚠️ needed · 🔄 local tracking stale, fetch+reset before any sync · ❓ unknown (`--no-fetch`) · 🏁 merged |
| merge | one GraphQL query: `mergeable`, `mergeStateStatus`, `isDraft`; gh stack `isMerged`/`isQueued` | ✅ mergeable now · 🚧 blocked (conflicting, behind, draft, queued, required checks, still computing) · 🏁 merged |
| CI | same query, `statusCheckRollup.state` of the head commit | ✅ green · 🌀 running · ⛔ failed, do not enter · — none |

`VERDICT` names the branch and the owner action for every problem (SHAs
appear there, not in the table). A bottom layer merely behind trunk is a note,
not a problem: do not rebase a layer whose CI could go green just to catch up
with trunk. The legend is printed once at the end. `--json` emits
`stacks[].rows[]`, `problems[]`, `notes[]`, `coherent` for agents that need to
branch on the result.

## Rules the skill enforces by convention

- This is **the** status call for stacks. Do not fan out `gh pr view` per
  branch; that costs N calls and still misses base coherence and needsRebase.
- The script is read-only. `gh stack sync` and `gh stack rebase` rewrite and
  force-push every layer; only the stack owner runs them, and only after this
  table shows `origin ok` on every layer (otherwise sync re-rebases stale local
  heads over someone else's push and turns PRs CONFLICTING).
- After ANY merge or rebase on a stack, run this again and reconcile before
  dispatching dev or QA against a layer.
- Draft PRs block `gh stack merge`; the table flags them.
