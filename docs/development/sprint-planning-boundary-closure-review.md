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

The resulting shape of one cross-boundary feature is three waves: contract,
layers, integration. Its critical path is three sprints whatever its size.

**Guard against the opposite extreme.** Cutting everything by layer is as
wrong as cutting everything by feature. It produces thin sprints, each paying
for a worktree, PR, QA pass and CI run, and it defers all integration risk to
one late checkpoint. The guidelines therefore plan in **tracks** and score a
plan on the shortest critical path with the fewest sprints:

- Independent changes inside one boundary are separate sprints. Two
  independent SQL schema changes are parallel siblings when their paths are
  disjoint, or one `gh stack` track when they share a file. Either way they
  run beside every other track.
- Independent features on disjoint paths stay vertical tracks.
- A layer cut is used only where two or more layer sprints are substantial.
  A thin contract with one implementer and one consumer is one sprint.
- Pass-through edits never get a sprint.
- Each cross-boundary feature integrates in its own integration sprint as
  soon as its layers close. One phase-wide checkpoint at the end is a finding.

`plan-scope-reviewer` raises `OVER-SPLIT` for the horizontal extreme, beside
`VERTICAL-SLICE` and `SERIAL-RISK` for the vertical one, and reports sprint
count with critical path and width.

## 4. Answers To The Open Questions

**Where does the integration checkpoint live?** In one `integration` sprint
per cross-boundary feature, at the end of that feature's track, before the
phase-ending review. They own composition-root wiring, CLI and end-to-end behaviour,
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
| `CLAUDE.md`, hardening examples and step files | Branch diagram and examples use the one naming convention. |

`closure_type` and `target_boundary` are now required variables in the
orchestration sprint-plan template. Nothing in code or tests renders that
template, so no gate breaks. Existing sprint docs are not migrated.

## 6. Audit Of The Plan-Hardening Loop

Question: did the skill or its agents put any emphasis on parallel sprints?
On develop, almost none. The whole loop carried one sentence, in the
guidelines: "Prefer parallel-safe splits where credible. Plan-scope-reviewer
verifies this."

| Loop part | Parallelism content on develop | After this PR |
|---|---|---|
| `SKILL.md` | none; hard stop says "split it" | expected result is the wave shape; serial split rejected |
| Step 1 task template (planner) | one line on relation tags | cut from boundary map, publish wave table |
| Step 2 `plan-scope-reviewer` | one check on relation rationale | boundary cut, `owned_paths`, critical path, width, two new findings |
| Step 3 task template (planner) | none; central question was production-ready split | central question is the boundary cut |
| Step 4 `critical-plan-reviewer` | none; `FALSE-CLOSURE` pushed toward full-stack sprints | closure judged per sprint type |
| Step 5 consistency template | none | guard: the pass must not make the plan more serial |
| Step 6 `quality-mgr` plan QA | none; `req-qa` could flag a boundary sprint as a coverage gap | sprint docs judged at their closure type |
| Round tables, steps 2 and 4 | track only finding counts | record critical path and width per round |
| `ruthless-boundary-qa`, `req-qa`, `arch-qa`, `schema-reviewer`, `boundary-guard` | none | unchanged |

Three structural observations go beyond wording.

- The loop measured one thing, finding counts, and drove them to zero. It
  never measured the plan's shape. A plan could pass every gate fully serial.
- Every reviewer in the loop can only add obligations. None could say "this
  plan is too serial". `plan-scope-reviewer` now can.
- The loop itself is serial with one author: six steps, `arch-ctm` writes,
  two background reviewers take turns, each with a three-cycle cap. Steps 2
  and 4 could review the same commit at the same time, because their scopes
  do not overlap. That change is not in this PR.

## 7. Naming Conventions

Found on develop: no single rule, and four forms in use.

| Thing | Forms found |
|---|---|
| Plan directory | `phase-AA` to `phase-Z` upper case, `phase-af` onward lower case |
| Phase plan file | `phase-bb-plan.md` in use, `plan-phase-X.md` in the hardening examples |
| Sprint doc | `sprint-BB.4-task-start.md` (upper case, dot); older `sprint-aj-6-...` |
| Sprint branch | `feature/pN-s1-...` in `CLAUDE.md`, `feature/pAJ-s6-...` in the triage fallback, `feature/bb7-docs` in practice |
| Plan branch | `plan/phase-ap` and `docs/phase-ba-plan` |
| Phase branch | `integrate/phase-bb`; older `integrate/phase-AK` |

Now fixed in one table, "Naming" in the guidelines, summarised in
`CLAUDE.md` and checked by `plan-scope-reviewer` as a `NAMING` finding:

- everything lower case; phase id `bc`, sprint id `bc-4`
- plans land in `docs/plans/phase-<phase>/` as `phase-<phase>-plan.md` and
  `sprint-<phase>-<n>-<slug>.md`
- phase work on `integrate/phase-<phase>`
- sprint work on `sprint/<phase>-<n>-<slug>`, same slug as the sprint doc;
  fix layers on `fix/<phase>-<n>-<slug>`
- plan written on `plan/phase-<phase>`

The triage and stack scripts need no change. They match `integrate/phase-`
without regard to case and read each sprint's declared branch from
`.sprints/`. The old `feature/pAJ-s6` inference is a fallback for phases with
no declared branch. Existing phases are not renamed. `.sprints/<PHASE>/`
directories and triage sprint IRIs (`BB`, `BB6`) are data identifiers, still
upper case, and are left for a ruling.

## 8. Decisions For Rand

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
4. **Upper-case triage identifiers.** `.sprints/BB/` and sprint IRIs such as
   `BB6` are read by four scripts and their tests. Lower-casing them is a code
   change with a migration, so it is not in this PR. Say if you want it.
5. **Plan branch target.** The plan PR still targets `develop` from
   `plan/phase-<phase>`, so the plan is reviewable before the phase branch
   exists. If you want the plan written on `integrate/phase-<phase>` itself,
   that is a one-line change to the table.
6. **Follow-ups not in this PR:** the boundary-map script, the triage boundary
   root, `quality-mgr.md` and `qa-template.xml.j2` wording for per-crate
   sweeps in boundary sprints, and a trial of the new shape on the next
   phase plan with critical path and width recorded for comparison.
