# Sprint Planning Review: Boundary Closure And Parallel Waves

Date: 2026-09-18. Author: fenix, for Rand.

This review answers the problem statement "Sprint Closure Definition Is
Forcing Vertical Slicing and Blocking Parallel Execution". It records the
evidence, the assessment, what this change set edits, and the decisions that
remain with Rand.

## 1. Verdict

The problem statement is correct, and the evidence is stronger than it
claims. Three things produce the serial plans, not one:

1. **The closure definition.** The guidelines never said "demoable". The
   forcing text was the Production-Ready Expectation, which banned
   "boundary-only completion when runtime behavior is still open". A sprint
   that closed one crate against its contract was, by definition, a defect.
2. **The reviewers.** `plan-scope-reviewer` raised `NON-PROD` and
   `critical-plan-reviewer` raised `FALSE-CLOSURE` (never Minor) against
   exactly that shape. Plan hardening therefore pushed every plan toward
   full-stack sprints, round after round.
3. **The only remedy was "split".** Plan hardening has no step that asks for
   a dependency graph, a wave table or a parallel width. When a sprint was too
   big, it was split into two smaller feature sprints. Both still touched the
   same crate stack, so the split added a `must_follow` edge. Hardening made
   plans longer and more serial.

`parallel_safe` already existed. Nothing made the planner start from
boundaries, so it was almost never reachable.

## 2. Evidence

### Plans (docs/plans, phases AX, AY, AZ, BA, BB)

- No sprint in any of the five phases was scoped to one crate.
- The share of sprints on a serial `must_follow` chain ran from 64% to 100%.
  Phase AZ was fully serial.
- About a quarter of `must_follow` rationales are same-file collisions, not
  contract dependencies. BB.4 to BB.5 is one: both edit
  `herdr_task_start.rs`.
- Phase BB fixed its interfaces up front: the `TaskTransition` enum, ADR-061
  and the boundary manifests. BB.4, BB.5 and BB.6 still ran in series through
  `atm`, `atm-storage-rusqlite` and `atm-http-runtime`, and each carried its
  own colima live-daemon acceptance. The contract was ready for parallel
  work. The sprint cut did not use it.
- The only parallel pairs in BB were crates versus skills versus docs.
- End-to-end gates sit in middle sprints. Rand already ruled against this
  once, on 2026-09-05: "I don't want a sprint including live evidence"
  (AX.7 superseded).

### Git history (Phase BB)

- PR #1452 is titled as the collapse of BB.1 to BB.5 plus every fix layer
  into one branch. The serial stack became one unit in practice.
- 24 fix layers and 30 PRs landed in under 12 hours.
- `atm-core` was touched by 9 PRs and `atm-http-runtime` by 8. Each crate
  was reopened again and again, which is the double cost the problem
  statement describes.

### ATM message record (since 2026-09-09, by workflow tag)

| Event | Count |
|---|---|
| dev-start | 36 |
| fix-start | 41 |
| qa-start | 45 |
| qa-approved | 19 |
| qa-rejected | 24 |
| plan-stage messages | 1 |

More fix rounds were started than dev rounds. More QA rounds were rejected
than approved. Every QA round went to a single `quality-mgr`. One message in
the whole window carried a plan-stage tag, so plan hardening leaves almost no
trace in the record it is supposed to feed.

Two agent-reported figures were discarded as unverifiable: a "3.9x slower"
estimate and free-text match counts that hit the query limit.

## 3. Assessment Of The Proposed Direction

The proposed closure, "the target crate boundary is trait-complete and
lint-clean against the ADR-fixed contract", is adopted with three changes.

- **"Trait-complete" is not enough by itself.** A crate can implement every
  method and still be wrong. Closure is "contract tests pass", using the
  pattern that already exists in `writer_contract_test!`
  (`crates/atm-storage-rusqlite/tests/task_ledger_writer.rs`). The contract
  tests are written once, in the contract sprint, and every implementer runs
  them.
- **Consumers need a closure too.** A crate above a contract closes when its
  own tests pass against the test double at the manifest's
  `allowed_test_double_paths`.
- **A contract sprint must come first.** Parallel layer sprints are only safe
  when the interfaces, the shared registry files and the test doubles already
  exist. That sprint is short, and it is the only thing the layer sprints
  follow.

The resulting shape is three waves: contract, layers, integration. Critical
path is three sprints whatever the phase size. Width is the number of
boundaries the phase touches.

## 4. Answers To The Open Questions

**Where does the integration checkpoint live?** In one or more
`integration` sprints in the final wave of the phase, before the phase-ending
review. They own composition-root wiring, CLI and end-to-end behaviour,
colima and smoke procedures, and user docs. Every feature-level acceptance
criterion lives there exactly once. The phase-ending review stays as the
full-reviewer gate on top.

**Does every feature decompose to crate boundaries?** No. Three cases do
not: the boundary does not exist yet, a schema migration must change writer
and reader in one commit, and a defect fix. These are allowed as vertical
sprints with a recorded `vertical_rationale`. "The feature needs all these
layers" is not a rationale. An unexplained multi-boundary sprint is a
`VERTICAL-SLICE` finding.

**Does the planner need the crate dependency graph as explicit input?** Yes.
The planner now builds a boundary map from `boundaries/<crate>/*.toml`
before cutting any sprint, and publishes a wave table with critical path and
width. A script that prints the map from the manifests is a worthwhile
follow-up. It would reuse the parsing in `.just/lint_boundaries.py`.

**Do triage TTL roots need adjusting?** Yes, as a follow-up. Acceptance
criteria in contract and boundary sprints are now rooted at
`boundary:<boundary_id>`. Criteria rooted at `req:<ID>` belong to integration
sprints. The sprint docs carry the root today. The triage Turtle schema does
not yet have a boundary root, so a finding in one crate can still reopen
criteria owned by another. That schema change is not in this PR.

## 5. What This Change Set Edits

| File | Change |
|---|---|
| `.claude/skills/plan-hardening/sprint-planning-guidelines.md` | Rewritten. Boundary map, three sprint kinds, closure types, `owned_paths`, `parallel_safe` as default, wave table, vertical exceptions, split along boundaries. |
| `.claude/agents/plan-scope-reviewer.md` | Checks the new shape. New findings `VERTICAL-SLICE` and `SERIAL-RISK`. Output reports waves, critical path and width. Never asks for a serial split. |
| `.claude/agents/critical-plan-reviewer.md` | `FALSE-CLOSURE` is judged at the level each sprint claims. Must not ask for end-to-end proof inside a contract or boundary sprint. Checks the contract sprint is complete enough for parallel work. |
| `.claude/skills/plan-hardening/SKILL.md` and task templates 01, 02 | Hardening must not lengthen the critical path. The planner is told to cut from the boundary map. The central question of scope hardening is now the boundary cut. |
| `.claude/skills/codex-orchestration/sprint-plan.md.j2`, `docs/templates/sprint-plan.md.j2` | New fields `closure_type`, `target_boundary`, `owned_paths`, `vertical_rationale`. Acceptance criteria carry a root. |
| `.claude/skills/codex-orchestration/SKILL.md` | QA scope follows the closure type. |

`closure_type` and `target_boundary` are now required variables in the
orchestration sprint-plan template. Nothing in code or tests renders that
template, so no gate breaks. Existing sprint docs are not migrated.

## 6. Decisions For Rand

1. **QA once on top of the stack, or one QA per layer sprint?** The ruling of
   2026-09-13 says QA and CI gate only the top of one append-only stack. The
   target workflow in the problem statement has a dev and QA pair per layer
   sprint. These conflict. One stack is also a serial structure: ten parallel
   sprints cannot each be "cut from the current top". Recommendation: layer
   sprints branch from the contract sprint's tip as siblings, each gets one
   narrow QA pass scoped to its one crate, and the stack rule keeps applying
   inside a sprint's own fix layers and to the integration wave. This PR does
   not change `CLAUDE.md` or `gh-stack-guidelines.md`. It needs your ruling.
2. **Contract sprint versus the no-unused-code rule.** A contract sprint
   lands traits and test doubles before any production consumer exists. The
   guidelines handle this by requiring that the phase leaves no contract
   without a production consumer at integration. If the unused-code lint
   fires inside the phase branch, it needs a phase-scoped allowance.
3. **QA capacity.** All 45 QA rounds went to one `quality-mgr`. Ten parallel
   sprints need several QA coordinators, or the queue moves from dev to QA.
4. **Follow-ups not in this PR:** the boundary-map script, the triage boundary
   root, `quality-mgr.md` and `qa-template.xml.j2` wording for per-crate
   sweeps in boundary sprints, and a trial of the new shape on the next
   phase plan with critical path and width recorded for comparison.
