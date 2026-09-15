# Phase AX Post-Mortem

Phase AX (nudge templates on every backend, task-state tracking) merged to
`develop` 2026-09-06T23:14:05Z via PR #1253, merge commit `98661ea18`
(parents `9a1e242d1` develop + `247bb1340` integrate/phase-ax tip). Seven
sprints (AX.1-AX.6 complete, AX.7 superseded 2026-09-05). Prepared per
`.claude/skills/triaging-findings/references/post-mortem.md`.

**Phase-level outcome: `integration_review_passed`** — quality-mgr's
phase-ending regression recheck (QA-AX-1253-PREMERGE) on stack-top
`556bb094b` returned PASS 0/0/0
(https://github.com/randlee/atm-core/pull/1264#issuecomment-5562504738);
independently re-verified by team-lead against the final merged head
`247bb1340` before #1253 landed. `next-dev-task AX .sprints/AX` returns
`DONE` with zero open findings on `develop` post-merge.

## AX7 data-gap ruling

`triage_report.py` requires an authoritative QA run per `structure.ttl`
sprint and has no superseded-sprint handling, so AX7 (live Herdr dogfood
evidence, superseded 2026-09-05 when live-proof moved to release readiness)
reports `QA_RUN_MISSING`.

**Ruling**: keep AX7 in `structure.ttl` marked `superseded` (already the
case) rather than deleting it — the sprint was real planning work that was
legitimately re-scoped, not a phantom entry, and removing it would erase
that decision trail. No QA run will be authored for it; the `QA_RUN_MISSING`
report line is expected and documented here, not a defect to chase. Classified
below as `qa_process_improvement` (tooling gap, not phase-blocking).

## Finding Families

### 1. Finding-ID collisions across concurrent QA rounds

- **Ids**: PR #1214's own `RBP-F001`/`RBP-F002`/`RSH-002` vs. PR #1232-round
  `RBP-F001`/`RBP-F002`/`RSH-002`/`RSH-003` vs. already-closed AX1
  `RBP-F001.ttl`/`RBP-F002.ttl`; separately, req-qa's raw `ATM-QA-001` in the
  #1236 round collided with the existing `ATM-QA-004` series and was
  renumbered `AXPE-QA-114` before filing.
- **Pattern**: reviewer agents mint finding ids independently per-round with
  no shared registry check, so unrelated defects on different branches/rounds
  land under identical ids. Every occurrence required manual disambiguation
  (an `AX-PE2-` / `AXPE-` prefix) before dispatch — caught by the reviewers
  themselves each time, but only after the fact.
- **Root cause**: missing lint / missing gate (no static ID-uniqueness check
  at finding-render time).
- **Classification**: `new_lint`, `qa_process_improvement`.
- **Recommended action**: `triage-record.ttl.j2` render (or the
  `qa-triage` agent) should check `.triage/<phase_id>/findings/` for an
  existing file at the proposed id and reject/re-mint rather than relying on
  reviewers to notice a collision in prose. A round-scoped prefix convention
  (e.g. `<phase>-PE<n>-`) is the cheaper interim fix if the allocator isn't
  built first.
- **Owner**: team-lead (skill change) / arch-ctm (implementation).
- **Target artifact**: `.claude/skills/triaging-findings/triage-record.ttl.j2`,
  `.claude/agents/qa-triage.md`.

### 2. RULE-003 (1000-production-line cap) recurring near-cap violations

- **Ids**: `ARCH-001`/`ARCH-002` (AX.1, one pre-existing/one newly introduced);
  the RULE-003 hand-count dispute in the #1232 round (arch-qa's manual count
  wrong by 4 lines, independently disproven by rust-qa-agent's script);
  `herdr_queue_wake.rs` hitting 1006 lines after the final develop-resync
  merge, fixed post-merge via a private extraction (PR #1266).
- **Pattern**: the cap is hit repeatedly near the boundary rather than being
  checked before a file grows into it, and at least once a reviewer without
  Bash access hand-counted lines and got it wrong (asserted Blocking on a
  false positive).
- **Root cause**: missing gate earlier in the loop (cap is only ever
  discovered at QA time or later) + a reviewer capability mismatch (manual
  count treated as authoritative when only the script is).
- **Classification**: `qa_process_improvement`, `planning_process_improvement`.
- **Recommended action**: (a) reviewers lacking Bash/script execution must
  report a suspected RULE-003 breach as "needs script verification," never
  as an asserted severity — quality-mgr/rust-qa-agent cross-checks before
  it's treated as Blocking; (b) consider running
  `.just/check_line_counts.py` as a pre-dispatch sprint-doc check when a
  sprint's target file is already within ~100 lines of the cap, so the split
  is planned rather than reactive.
- **Owner**: team-lead (process), quality-mgr (reviewer instructions).
- **Target artifact**: `.claude/skills/quality-management-gh/SKILL.md`,
  `.claude/skills/graph-orchestration/SKILL.md` (dev-task template).

### 3. Stale local git state trusted without an explicit fetch+verify step

- **Ids**: `boundary-guard`'s FAIL on a 401-commits-behind local `develop`
  checkout (stale-checkout-review incident); the `base_ref_changed` `/timeline`
  false-negative on PR #1253's brief base retarget; arch-ctm's
  reported-then-retracted "fast-forward to develop" confusion, ultimately
  traced to a working-directory mistake in his primary checkout rather than a
  ref/topology problem.
- **Pattern**: three independent incidents of an agent (subagent or teammate)
  drawing a conclusion from git state without confirming it was reading the
  fetched/intended ref, each requiring a correction after team-lead
  re-verified with explicit full-SHA `git merge-base --is-ancestor` checks
  post-fetch.
- **Root cause**: no standing requirement that a "what does branch X
  currently do" claim be backed by a fresh fetch + fully-qualified-ref
  verification; this discipline currently lives only in team-lead's own
  practice/memory, not in any shared skill or agent instructions.
- **Classification**: `qa_process_improvement`, `new_lint` (candidate: a
  reviewer-agent preflight that runs `git fetch` and asserts
  `git status --short --branch` shows no `[behind]` before reading).
- **Recommended action**: add an explicit "fetch and confirm you're reading
  the intended ref, not a stale local pointer" requirement to reviewer agent
  prompts and to `team-protocol.md`, rather than leaving it as tribal
  knowledge re-derived per incident.
- **Owner**: team-lead.
- **Target artifact**: `docs/team-protocol.md`,
  `.claude/agents/boundary-guard.md` and other reviewer agent prompts.

### 4. Direct push to an integration branch during an in-flight `gh stack` operation

- **Ids**: team-lead's own `10b26cf1e` push to `integrate/phase-ax`
  (routine `events.ttl` Completion append) landing ~1 minute after fenix's
  `gh stack sync` had computed rebase bases, forcing a fallback to the async
  merge API instead of a clean stack merge.
- **Pattern**: no explicit, always-announced freeze/lift protocol existed
  around `gh stack sync`/`merge` windows; a "routine" commit was pushed
  without checking for in-flight stack activity.
- **Root cause**: `merge_forward_process_improvement` — missing explicit
  coordination step, not a tooling gap (gh-stack itself behaved correctly;
  the process around it didn't).
- **Confirmed cost**: Rand-confirmed 40-60 minutes of added CI time.
- **Classification**: `merge_forward_process_improvement`,
  `qa_process_improvement`.
- **Recommended action**: codify as a hard rule: once a `gh stack sync`
  targeting a shared integration branch has started, no other agent pushes
  to that branch until the operator (whoever is running the stack) posts an
  explicit "freeze lifted" / merge-complete message. Already applied
  operationally this phase (see `feedback_no_push_during_stack_merge.md`,
  `feedback_freeze_trunk_while_stack_lands.md`); this should graduate from
  memory into a written skill rule.
- **Owner**: team-lead.
- **Target artifact**: `.claude/skills/graph-orchestration/SKILL.md` or a new
  `docs/team-protocol.md` section on integration-branch freeze windows.

### 5. Benchmark evidence `source_revision` citing an unresolvable local SHA

- **Ids**: PR #1263's accepted benchmark campaign (`20260906T194537Z`)
  recorded `source_revision: d53715bd5...` — a SHA that resolves nowhere
  (a local pre-amend/pre-rebase commit that never reached origin).
- **Pattern**: the benchmark harness captures `source_revision` from local
  HEAD at run time, before the standard "iterate locally, fix, then push"
  autonomy loop's final push/amend; this can silently detach the evidence
  record from any real commit.
- **Root cause**: test/harness gap — capture point is wrong, not a data
  fabrication.
- **Classification**: `test_hardening`.
- **Recommended action**: capture `source_revision` from the actually-pushed
  HEAD (post-push), or amend/append it to the evidence record after push,
  never from local HEAD at benchmark time.
- **Owner**: arch-ctm (harness change) via team-lead dispatch.
- **Target artifact**: benchmark harness evidence-capture code (`just
  benchmark-report` path) — see `feedback_benchmark_source_revision_must_resolve.md`.

### 6. Individual deferred findings (issue #1243 — non-blocking, Rand-waived 2026-09-05)

| Finding | Severity | Classification | Target artifact |
|---|---|---|---|
| `AXPE-ARCH-003` — `async-task-ledger-reader.toml`/-sqlite have no boundary-doc section, unlike TaskStore | important | `boundary_update` | `docs/atm-storage/boundaries.md` |
| `AXPE-RBQ-003` — peer provenance independently re-derived at 3 sites instead of threading the one resolved `ValidatedWriteProvenance` | important | `architecture_update` | `crates/atm-core/src/write/pipeline.rs`, `send/async_persistence.rs:201,286` |
| `AXPE-RSH-001` — Herdr queue-wake pump task has no termination/liveness guard (unlike the HTTP listener's `ServerTaskTerminationGuard`) | important | `architecture_update`, `test_hardening` | Herdr pump module (sibling guard pattern, RULE-003-aware) |
| `AXPE-RSH-002` — `last_tick_at` surfaced but no staleness check flags a stalled pump | important | `test_hardening` | Herdr doctor/readiness check |
| `AXPE-RBP-001` — escalation recipient addresses cross the TaskStore trait boundary as raw strings; ADR-062 says validated, enforced one layer up only | important | `architecture_update` | `docs/adr/ADR-062-task-state-machine.md` amendment or a validated newtype at the trait boundary |
| `AXPE-RBP-002` — duplicate `state_name`/`event_name`/`outcome_name` string mappings instead of calling the canonical `.as_str()` | minor | `no_systemic_followup` (mechanical cleanup) | `atm-storage-rusqlite/task_store.rs`, `writer/task_ops.rs` |
| `AXPE-QA-115` — `tools/bootstrap.toml` pins rust 1.94.1 vs. cargo-shear 1.13.3 needing >=1.95, blocking local `just validate`/`bootstrap` | important | `planning_process_improvement` (tooling debt, tracked separately per `project_bootstrap_toolchain_version_debt.md`) | `tools/bootstrap.toml` |
| `AXPE-QA-116` — commit message overstates a byte-for-byte evidence restore as a "fix" | minor | `no_systemic_followup` | none (cosmetic; not amended to avoid invalidating a QA-passed head) |

All eight are tracked in GitHub issue #1243 and remain open there as
follow-up work; none block Phase AX or future phases.

### 7. Phase-end review run against the wrong branch

- **Ids**: the `merge/develop-into-phase-ax` (#1232) phase-end review round,
  where two reviewers (req-qa, ruthless-boundary-qa) hit a branch-topology
  gap because the review ran against the freshness-merge branch rather than
  the branch that would actually become the phase PR head.
- **Classification**: `qa_process_improvement`.
- **Recommended action**: run the phase-end recheck only against whichever
  branch will actually become the phase PR head, after all stacks/merges
  targeting it have landed — not against an intermediate freshness-merge
  branch. Applied correctly on the final #1253 premerge recheck this phase.
- **Owner**: team-lead.
- **Target artifact**: `.claude/skills/triaging-findings/references/post-mortem.md`
  (Required Integration Review section — make the "final branch, not an
  intermediate merge branch" requirement explicit).

## Summary

Two families (#1, #4) are genuine process gaps with real, confirmed cost
this phase (finding-id collisions requiring repeated manual disambiguation;
a 40-60 minute CI delay from a stack-window push) and should graduate from
session memory into written skill/protocol changes. Two families (#2, #3)
are recurring-but-caught patterns worth a small upstream check. One (#5) is
a harness bug with a clear fix. Family #6 is eight individually-tracked,
non-blocking design/tooling debt items already filed under issue #1243.
Family #7 self-corrected within the phase.
