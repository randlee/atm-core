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
/gh-stack-view [--no-fetch] [--no-pr] [--json]
```

Run from a worktree checked out on any branch of the stack (the main repo on
`develop` is never part of a stack and will error; never `git checkout` there).

```bash
cd ../atm-core-worktrees/<any-stack-layer>
python3 "$(git rev-parse --show-toplevel)/.claude/skills/gh-stack-view/scripts/gh_stack_view.py"
```

The script path resolves inside the current worktree; if the skill is not on
that branch yet, point at the main repo's copy instead:
`python3 <main-repo>/.claude/skills/gh-stack-view/scripts/gh_stack_view.py`.

Exit code: `0` coherent, `1` problems listed, `2` not in a stack.

## Output

The script renders everything. **Paste its output verbatim.** The agent makes
no rendering decisions: no reformatting, no re-summarising the table, no
substituting its own per-branch lookups.

```
stack: integrate/phase-aw @ 0f4ab1be2  (current: fix/aw-pool-read-migration)

| L | branch | PR | head | base | sync | merge | CI |
|---|---|---|---|---|---|---|---|
| 1 | fix/aw-pool-consolidate | #1242 | a60779787 | 0f4ab1be2 | ✅ | 🚧 | 🌀 |
| 2 | fix/aw-pool-read-migration | #1244 | 913dc9413 | a60779787 | ✅ | 🚧 | 🌀 |

VERDICT: ✅ COHERENT - every base == parent head, every head pushed and on its PR
```

| Column | Source | Icons |
|--------|--------|-------|
| head / base | `gh stack view --json` | local stack-tracking SHAs |
| sync | computed from `base`, `origin/<branch>` (one fetch) and PR `headRefOid` | ✅ base==parent head and local==origin==PR · 🔄 local/origin/PR heads differ · ⚠️ needs rebase · ❓ unknown (`--no-fetch`) |
| merge | one GraphQL query: `mergeable`, `mergeStateStatus`, `isDraft`; gh stack `isMerged`/`isQueued` | 🚀 clean · 🚧 blocked/unstable · ⏪ behind · ❌ conflicting/dirty · ⏳ computing · 📝 draft · ◎ queued · 🏁 merged |
| CI | same query, `statusCheckRollup.state` of the head commit | ✅ success · ❌ failure · 🌀 pending · — none |

`VERDICT` lists every problem with the owner action. A bottom layer merely
behind trunk is a note, not a problem: do not rebase a layer whose CI could go
green just to catch up with trunk. The legend is printed under every table.

## Rules the skill enforces by convention

- This is **the** status call for stacks. Do not fan out `gh pr view` per
  branch; that costs N calls and still misses base coherence and needsRebase.
- The script is read-only. `gh stack sync` and `gh stack rebase` rewrite and
  force-push every layer; only the stack owner runs them, and only after this
  table shows `origin ok` on every layer (otherwise sync re-rebases stale local
  heads over someone else's push and turns PRs CONFLICTING).
- After ANY merge or rebase on a stack, run this again and reconcile before
  dispatching dev or QA against a layer.
- `--json` emits the merged rows (`rows[]`, `problems[]`, `notes[]`,
  `coherent`) for agents that need to branch on the result.
- Draft PRs block `gh stack merge`; the table flags them.
