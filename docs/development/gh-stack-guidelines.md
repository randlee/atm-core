# gh-stack Guidelines for Phase Work

Best practices for running a phase as a GitHub stack (`gh stack`) of sprint
and fix layers above `integrate/phase-N`. Every rule below was paid for during
Phases AX, AZ, and BA; the "Why" lines cite the incident. The orchestrator
(fenix or team-lead) owns the stack; devs own exactly one layer each.

Vocabulary: **trunk** = `integrate/phase-N` (or `develop` for release-fix
stacks); **layer** = one branch with one PR whose base is the layer below;
**bottom** = closest to trunk; **top** = the landing head; **writer** = the
single agent allowed to push a layer; **frozen** = a layer whose minimum
functionality is complete and which nobody touches again.

## 1. Shape the stack for parallel work

1. **Stack the next sprint the moment the critical code exists.** When a
   sprint's types, schema, and core read/write paths are pushed and only
   test/lint/QA rounds remain, create the next sprint's worktree as a layer on
   that head and deploy its dev. Trigger check: "could the next sprint compile
   against this head?" If yes, deploy. *Why:* serial sprint-after-merge left
   two devs idle for an hour in BA.2.
2. **One linear chain, never a fork.** Two sprints based on the same parent
   (BA.4 and BA.5 both on BA.3) cannot both be stacked; one of them has to be
   re-based on the other later, and `gh stack merge` refuses forks. Order
   sibling sprints by dependency and chain them, or run the independent one
   as its own stack on trunk.
3. **Dependent PRs join the stack when they open, not when CI is green.**
   The stack is what makes each layer's CI run against its real base. Holding
   a PR out of the stack "until it is green" hides the state it was meant to
   show. *Why:* Phase AX #1262/#1264/#1266 sat unstacked with a red layer that
   a sibling PR already fixed.
4. **PR on the first push of every layer, fix layers included.** A branch
   without a PR has no CI and cannot be judged. Open the PR (base = the layer
   below) the moment the first push lands, then link it. Add as many stacked
   fix layers above the top sprint as the work needs.
5. **Fix and cleanup work goes to the top of the stack, never to a
   mid-stack branch.** Non-blocking findings from every layer are collected
   (triage records with `promote_to_branch` = highest open layer) and fixed
   ONCE on a new top layer, with ONE QA pass there. Never run per-branch fix
   rounds; never wait for a lower layer's QA or CI before the top layer moves.
   *Why:* per-branch rounds in BA.4/BA.5 cost 2–3 hours and multiplied QA
   cycles.

## 2. Own and move layers safely

6. **One writer per layer.** The orchestrator opens PRs, links, syncs, and
   lands; the layer's dev pushes; nobody else. Devs never open PRs and never
   push to another layer, to trunk, or to `.triage`/`.sprints` (team-lead is
   the sole writer there).
7. **Rebase at task start, by the writer, once per task.** The first step of
   a task on layer N is `git fetch && git rebase --onto origin/<parent>
   <recorded-parent-base> <layer>` followed by `push --force-with-lease`; the
   orchestrator records the new parent base sha in the ledger. Between tasks,
   layers do not move. Frozen intermediate layers are rebased by the
   orchestrator in one pass when the layer below them freezes.
   *Why:* per-push rebases cascade force-pushes under live agents;
   merge-forward-only leaves tangled history.
8. **Never rewrite a layer that has children.** If a lower layer must be
   rewritten (BA.3's rewrite after BA.4/BA.5 branched), every child must be
   rebased in the same pass before anyone branches again. Otherwise the
   children hold stale copies, the merge-base regresses, every downstream
   diff shows phantom "regressions", and the stack can no longer land
   linearly (§16).
9. **Freeze means freeze.** "As soon as the minimum functionality of a branch
   is complete for closure, do not touch that branch again." Red CI on a
   frozen layer is not fixed there; the top layer's tree is what reaches
   trunk, so only the top layer's CI and QA gate the landing. A file copy of
   frozen code on a live layer may be edited (fold the change into the
   finding it serves).
10. **Merge-forward only when a layer's parent is frozen and the layer is
    live.** The merge is one commit by the layer's writer; the parent wins
    where copies differ; failures exposed by the merge are diagnosed test
    versus runtime against the sprint doc, never silenced. (BA: seven BA.3
    fixtures were stale against the documented tick order after the merge;
    they were re-fitted on a new layer, no invariant weakened.)

## 3. Link, sync, and land

11. **`gh stack link` always needs `--base <trunk>` and PR numbers.** Without
    `--base` the bottom PR is retargeted to the repository default branch
    and locked there. Pass PR numbers, not branch names, from the main repo
    (branch names push the local ref, which may be another agent's unpushed
    state). Verify every base with `gh pr view --json baseRefName` afterwards.
12. **`gh stack link <stack#> <pr>` appends on TOP and retargets that PR to
    the current top.** To insert a layer in the middle: `gh stack unstack
    <stack#>`, `gh pr edit <pr> --base <intended parent>`, then
    `gh stack link --base <trunk> <all PRs bottom→top>`. This creates a NEW
    stack number; merged PRs drop out; re-verify every base.
13. **Stacked PRs receive no CI after they are linked.** `ci.yml` runs on
    `pull_request` events for trunk-family base branches only; a stacked PR's
    base is another feature branch, so pushes, close/reopen, and syncs fire
    nothing. Before landing, open a plain **CI-trigger PR** `top-head →
    trunk` (not linked into the stack), wait for its run on the exact landing
    sha, then close it. Check-runs are per commit, so the ruleset's required
    checks are satisfied for the stack. Expect zero check-runs on lower
    layers and do not chase them.
14. **Pre-push gates are the CI, locally.** The orchestrator runs the full
    gate list on every reviewed head before accepting a push: fmt, clippy
    `-D warnings`, workspace tests, the `cli_surface` feature test,
    `.just/check_line_counts.py` (RULE-003), `.just/lint_boundaries.py`,
    `scripts/check-function-length.py --base-ref origin/<trunk>`,
    `scripts/check-nudge-taxonomy.py`, `.just/check_read_concurrency_gates.py`,
    `just lint identities`, `just lint adr-index`, and `just lint manifests`
    (the three `run_lint.py` targets also cover doc-only pushes). *Why:* BA.3
    froze green on nine gates and went red in CI on the tenth (identity
    literals).
15. **Freeze the trunk while the stack lands.** After the final sync,
    nobody pushes to trunk — no triage records, no TTL events, no sync
    merges — until the landing is confirmed. Send an explicit FREEZE to every
    trunk writer and get an ack. *Why:* Phase AX: one Completion-event push
    a minute after the sync made the stack "out-of-date", forced the async
    API path, and restarted 40–60 minutes of CI on every layer.
16. **Land with `gh stack merge <stack#> --yes --merge`; if it refuses a
    non-linear stack, land the top PR.** Merge commits only, never squash.
    When a lower layer was rewritten (§8), `gh stack merge` reports "PR #X's
    branch is not a linear descendant of PR #Y's branch". Do NOT fall back to
    sequential `merge-async` merges: GitHub rebases each child branch after
    its parent merges, which rewrites frozen layers. Instead: `gh stack
    unstack`, `gh pr edit <top> --base <trunk>`, `gh pr merge <top> --merge`.
    Verify every lower head is an ancestor of the new trunk head; GitHub
    marks the lower PRs MERGED by itself and the merge commit carries the
    whole history.
17. **After landing, reset local refs to origin; never force-push a landed
    branch.** If a layer still has to merge separately (`PUT
    pulls/N/merge-async` with the full 40-char head sha), fetch and
    `reset --hard origin/<child>` in the child worktree afterwards.

## 4. QA and triage on a stack

18. **QA diffs a stacked PR against the base commit it actually merged,
    not the moving remote base.** Pin `base_ref` = `git merge-base <base>
    <head>` (or the second parent of the last merge-forward) in the QA vars
    and review that three-dot range. *Why:* BA.4 QA-1 reported two
    "branch-introduced" blockers that lived entirely in a stale BA.3 copy.
19. **Pin every review head; read docs and code with `git show <sha>:<path>`.**
    A dev worktree is single-writer and moves; reviewers use a detached
    scratch worktree or `git show`. A verdict on a stale checkout is
    rejected.
20. **One finding, one record across the stack.** A defect that recurs on a
    downstream layer is the same finding: extend its scope with
    `promote_to_branch`, never file a second record. Records are keyed by
    finding and branch; moved shas are history, not truth.
21. **Doc-named test presence is not coverage.** Before accepting a push
    that claims doc-named tests, read every test body against its doc line;
    grep for alias one-liners and discarded results; prove a guard test can
    fail. *Why:* BA.3's first suite had 8+ alias bodies and a handoff test
    asserting the opposite of the design.
22. **Reply to every push.** Silence is the second stall source after codex
    idle; a codex dev does not resume without a message.

## 5. Landing checklist (copy into the ledger)

- [ ] every layer below the top is frozen and recorded with its head sha
- [ ] top layer: ten local gates green, QA PASS posted on the PR
- [ ] CI-trigger PR `top → trunk` green on the landing sha, then closed
- [ ] FREEZE sent to every trunk writer and acked
- [ ] `gh stack merge <stack#> --yes --merge` (or §16 fallback)
- [ ] every layer head is an ancestor of the new trunk head; all PRs MERGED
- [ ] "landed" sent; freeze lifted; held record commits pushed
- [ ] local refs reset; review-findings stack opened above trunk

Related: `.claude/skills/codex-orchestration/SKILL.md` (dispatch templates),
`.claude/skills/triaging-findings/SKILL.md` (records and promotion),
`docs/postmortems/phase-ba-postmortem.md` (the incidents behind these rules).
