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
joins them into one report. Fully merged stacks are hidden unless `--all`.

```bash
python3 .claude/skills/gh-stack-view/scripts/gh_stack_view.py --phase aw
```

Exit code: `0` all coherent, `1` problems listed, `2` no stack found (a stack
needs at least one layer checked out in a worktree; never `git checkout` in
the main repo to get one).

## Output

The script renders everything. **Paste its output verbatim.** The agent makes
no rendering decisions: no reformatting, no re-summarising, no substituting
its own per-branch lookups.

```
stack: fix/aw-pool-read-migration -> integrate/phase-aw @ 0e640b20a

| L | PR | sync | merge | CI |
|---|---|---|---|---|
| 1/2 | #1242 | ✅ | 🚧 | 🌀 |
| 2/2 | #1244 | ✅ | 🚧 | 🌀 |

VERDICT: ✅ COHERENT - every base == parent head, every head pushed and on its PR
```

| Column | Source | Icons |
|--------|--------|-------|
| L | layer / stack depth, bottom first | |
| sync | `gh stack view --json` `head`/`base`/`needsRebase`, `origin/<branch>` after one fetch, PR `headRefOid` | ✅ base==parent head and local==origin==PR · 🔄 local/origin/PR heads differ · ⚠️ needs rebase · ❓ unknown (`--no-fetch`) · 🏁 merged |
| merge | one GraphQL query: `mergeable`, `mergeStateStatus`, `isDraft`; gh stack `isMerged`/`isQueued` | 🚀 clean · 🚧 blocked/unstable · ⏪ behind · ❌ conflicting/dirty · ⏳ computing · 📝 draft · ◎ queued · 🏁 merged |
| CI | same query, `statusCheckRollup.state` of the head commit | ✅ success · ❌ failure · 🌀 pending · — none |

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
