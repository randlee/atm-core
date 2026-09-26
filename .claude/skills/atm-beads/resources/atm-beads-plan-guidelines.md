# ATM Beads Plan Guidelines

Use these rules when writing and hardening sprint plans. The plan is written
to beads, not to markdown files: every rule that names a phase plan or a sprint
doc applies to the phase bead or the sprint bead.

The goal of a sprint cut is **independent work**: sprints whose owned paths
do not intersect, so they run at the same time. The plan's objective is the
shortest critical path with the fewest sprints. Ten non-intersecting sprints
in parallel are preferred over five in series, and five are preferred over
ten when the extra five buy no parallel work.

Crate boundaries are the default place to cut when the repository records
them in boundary manifests and enforces them through its configured boundary
validation command. This is independence the architecture already checks
mechanically. A plan that cuts
every sprint by feature makes each sprint touch the same crate stack and
forces a serial chain. A plan that cuts every change by layer has the opposite
defect: thin sprints, a sprint-sized overhead on each, and all integration
risk deferred to one late checkpoint. Both extremes are findings. See
"Tracks And Balance".

## Core Rules

- The plan beads are authoritative.
- Downstream prompts may carry structured projections of plan-bead data, but
  they must not replace or narrow the plan beads.
- If QA cannot review directly from the sprint bead, the sprint bead is not
  hardened.

## Plan Hierarchy And Dependencies

- The phase is one bead, normally of type `feature`, with every sprint bead
  of the phase under it as a `parent-child` child.
- Each sprint is two beads: a dev bead and a sanity check bead. The dev
  bead's description, design, acceptance criteria and metadata carry what
  the sprint doc carried.
- The sanity check bead is blocked by its dev bead and blocks every dev bead
  that requires that sprint. What it verifies is defined in
  [`dev-sanity.md`](dev-sanity.md).
- A dev bead is blocked by the sanity check bead of each required
  prerequisite (`must_follow`). `parallel_safe` sprints have no dependency
  between them.
- A track is a `gh stack` on the phase integration branch. The stack is
  specified when the plan is written: each sprint bead's metadata records its
  `stack`, `layer` (1 = bottom), `branch`, `pr_target` and `worktree`, where
  `pr_target` is the branch of the layer below (the integration branch for
  layer 1).
- The plan is optimized for parallel execution, so waves are horizontal
  layers of code: contract, layers, integration. A bead is released as soon
  as all of its blockers are closed, that is when all its required work is
  complete; `bd ready` lists it then. Sprints with no prerequisites and
  disjoint `owned_paths` run in parallel.

## Plan From The Boundary Map

Before cutting any sprint, the phase bead's design records a **boundary
map**: every crate and boundary manifest the phase changes, the contract change at each
(trait methods, types, schema, wire fields, error codes), and the
`allowed_dependents` / `allowed_dependencies` edges between them, read from
the manifests. Sprints are cut from this map, never from the feature list.

A cross-boundary track has three kinds of sprint, in this order:

1. **Contract sprint** (wave 1, short). Fixes every interface the phase
   changes: trait signatures, shared types, schema DDL, wire fields, error
   codes, boundary manifest edits, the ADR, the test double at the manifest's
   `allowed_test_double_paths`, and the contract tests that any implementation
   must pass. It also owns every shared registry file the phase touches
   (`mod.rs` re-exports, `Cargo.toml`, manifest indexes). A phase that changes
   no contract skips it.
2. **Layer sprints** (wave 2, parallel). One sprint per boundary. An
   *implementer* sprint fills in one side of a contract inside one crate. A
   *consumer* sprint builds the code above a contract inside one crate, tested
   against the test double. Both depend only on the contract sprint, so they
   are `parallel_safe` with each other.
3. **Integration sprints** (last wave of each track). Composition-root
   wiring, end-to-end and CLI behaviour, smoke procedures, and
   user-facing docs. Every feature-level acceptance criterion of the phase
   lives in one of them, exactly once.

A feature that crosses several boundaries does not get a full-stack sprint.
It closes at its integration sprint, after the layer sprints in its
dependency chain have closed. A feature that lives inside one boundary, or
inside paths no other sprint owns, is simply one sprint.

## Tracks And Balance

A **track** is a chain of sprints that must be serial among themselves. Tracks
run in parallel with each other. The plan is a set of tracks, and the test for
independence is always disjoint `owned_paths`, never the shape of the slice.

- **Independent changes inside one boundary are separate sprints.** Two
  independent SQL schema changes are two sprints. If their `owned_paths` are
  disjoint (separate tables, separate migration files) they are
  `parallel_safe` siblings. If they must share a file (a migration index, one
  DDL module) they form one `gh stack` track, the second layer cut from the
  first, and that track still runs in parallel with every other track.
- **Independent features are already parallel.** Two features that touch
  disjoint crates or paths are two vertical tracks. Do not re-cut them into
  layers; nothing is gained.
- **Cut by layer only when it creates parallel work.** A cross-boundary
  feature is cut into contract, layer and integration sprints when two or
  more of its layer sprints are substantial enough to occupy separate
  developers at the same time. When the contract change is small, with one
  implementer and one consumer, a single two-boundary sprint with
  `vertical_rationale: "thin contract, one implementer, one consumer"` is
  cheaper and is correct.
- **No thin sprints.** Every sprint costs a worktree, a PR, a QA pass and a
  CI run. A pass-through edit (a re-export, a field threaded through a
  handler, a few dozen lines) is folded into the adjacent sprint that needs
  it and listed in that sprint's `owned_paths`. It does not get a sprint.
- **Integrate per track, not per phase.** Each cross-boundary feature has its
  own integration sprint, which starts as soon as that feature's layer
  sprints close. It does not wait for unrelated tracks. Integration sprints
  of different features are `parallel_safe` with each other when their
  `owned_paths` are disjoint. One phase-wide integration checkpoint at the
  very end is the horizontal extreme and is an `OVER-SPLIT` finding.
- **Contract sprints are per track too.** Independent features with
  unrelated contract changes each get their own small contract sprint, so
  no track waits on another track's interface work.

## Closure Types

Every sprint bead declares one `closure_type` and one `target_boundary` in its
metadata.

| `closure_type` | Closes when | Required validation |
|---|---|---|
| `contract` | every interface the phase changes is committed as signatures, types, manifests and ADR text; test doubles and contract tests compile; workspace builds | workspace build plus the repository's configured boundary validation |
| `boundary` | the sprint's side of the contract is fully implemented in its one crate; contract tests pass against it (implementer) or its tests pass against the test double (consumer); boundary lint is clean; no `todo!`, `unimplemented!` or stubbed branch remains | target-crate tests and lints, workspace build, and configured boundary validation |
| `integration` | every feature-level acceptance criterion of the phase passes through the real composition; the phase leaves no contract without a production consumer | full workspace validation plus the phase's end-to-end procedures |
| `docs` | the named documents match the shipped surface | doc lint |

A `boundary` sprint does not claim, test, or demonstrate behaviour that
passes through another crate. It lists that behaviour under "This Sprint Does
Not Close" in its description and names the integration sprint that closes it.

Acceptance criteria in `contract` and `boundary` sprints are rooted at the
boundary (`boundary:<boundary_id>` or the ADR section). Criteria rooted at a
requirement id (`req:<ID>`) belong to an integration sprint. A change inside
one crate must not reopen criteria owned by another crate's sprint.

## Production-Ready Expectation

Every listed deliverable must land at a production-ready level **for the
closure type the sprint claims**.

Do not allow:

- shape-only completion: a signature with no behaviour behind it, outside a
  `contract` sprint
- test-only completion
- a `boundary` sprint that leaves part of its side of the contract stubbed
- silent carry-forward of a committed deliverable
- false closure at phase level: any feature-level behaviour of the phase that
  no integration-sprint acceptance criterion owns

Closing a boundary while runtime reach through other crates is still open is
**not** false closure. It is the intended shape, provided the open behaviour
is named in "This Sprint Does Not Close" and owned by an integration sprint.

## Ownership And Dependency Relations

Each sprint bead's metadata lists `owned_paths` (globs). Within a wave no path may match
two sprints. Reviewers check this mechanically, not by reading goals.

List each related sprint as `must_follow` or `parallel_safe` with a rationale.

- `parallel_safe` is the default. It requires non-intersecting `owned_paths`,
  public contracts, artifacts, and ownership.
- `must_follow` is allowed only when the child consumes a named contract
  artifact (type, trait method, schema object, wire field) that the parent
  produces **and** that artifact cannot be hoisted into the contract sprint.
  The rationale names the artifact.
- "Both sprints edit the same file" is never a `must_follow` rationale. It is
  a split defect. Re-cut ownership so the file has one owner, move the shared
  file to the contract or integration sprint, or merge the two sprints.
- A `must_follow` edge needs concrete coupling: the same files/crates/public
  types, or the child consumes the parent's code. Shared release/version
  baseline alone is not coupling; handle it with a final integration step.
- Parallel tracks run as separate gh-stack stacks with named branches,
  worktrees, and assigned agents.

The phase bead's design publishes a **wave table**: each track, its sprints by wave,
their `target_boundary` and `owned_paths`, plus three numbers: **critical
path** (longest `must_follow` chain, in sprints), **width** (most sprints
running at once) and **sprint count**. The expected shape of one
cross-boundary track is three waves: contract, layers, integration. A
single-boundary or vertical track is shorter. Every `must_follow` edge that
lengthens the critical path past three needs a recorded reason a reviewer can
check. A re-cut that raises sprint count without shortening the critical path
or raising usable width is a regression.

## Process Artifacts

A plan may require a process artifact (manifest, inventory, ledger, receipt,
matrix, docs-consistency check, new CI gate) only if the sprint bead names:
its consumer, the capability it gates, the observed defect it prevents (not
speculative), and when it is retired. Otherwise leave it out. Prefer what
already enforces the property: the compiler, existing tests, existing CI,
git history. A plan-writing rule (e.g. single ownership of a contract) stays
a plan rule; it does not become a product CI gate.

Bead `status` describes the sprint, not the plan: an unimplemented sprint
bead is `open`, never `closed`.

## Vertical Exceptions

A sprint may span more than one boundary only with a recorded
`vertical_rationale`, for example: the boundary does not exist yet (then the
sprint's deliverable is to create it, and later sprints use it), a schema
migration that must change writer and reader in one commit, or a defect fix.
"The feature needs all of these layers" is not a rationale; that is what the
integration sprint is for. An unexplained multi-boundary sprint is a
`VERTICAL-SLICE` finding.

## Split Early

Split a sprint immediately when any of these are true:

- it owns more than one boundary without a `vertical_rationale`
- there is credible doubt that every committed deliverable can land at a
  production-ready level for its closure type in the same sprint
- it mixes closure types
- acceptance criteria would allow one deliverable to slip while the sprint
  still claims success
- the same deliverable is being planned more than once across multiple sprints

Split **along boundaries**, into siblings that can run in the same wave.
Splitting one overloaded feature sprint into two serial feature sprints makes
the plan slower, not safer. Do not preserve an overloaded sprint just to keep
the sprint count low.

## Sprint Bead Shape

Each sprint bead should have one authoritative list for:

- `requirements` (metadata): every REQ and NFR id the sprint implements or is
  constrained by, from the requirements index and crate requirements docs
  named in `.claude/project/quality-policy.md`
- `adrs` (metadata): every ADR id that governs the sprint, from the architecture index
  and crate architecture docs named in `.claude/project/quality-policy.md`;
  a new or amended ADR is also a deliverable
- deliverables, each naming the REQ/NFR id it serves (description)
- acceptance criteria, each with its root (acceptance criteria)
- owned paths (metadata)
- paths to delete, when applicable (description)
- required validation (acceptance criteria)

Do not restate the same checklist item in multiple sections with different
wording.

## Naming

One convention per repository, recorded under "Plan Naming" in
`.claude/project/quality-policy.md`: phase id form, sprint id form, and
phase, sprint and fix branch names. Its plan directory and file names do not
apply. Every sprint id and branch follows it. Rules that hold in every
repository:

- Phase ids are always lower case (`phase-m`) and run in order a, b, c, ...
  z, aa, ab, ac, ...
- Branch names, bead metadata values, labels and template variables are lower
  case. Prose and titles may use the upper-case display form.
- The slug is the same in the sprint bead title and the sprint branch name.
- Slugs use `a-z`, `0-9` and `-` only. No dots, no underscores, no capitals.
- All phase work happens on the phase integration branch. Sprint and fix
  branches are cut from it, or from the top of their track's stack, and their
  PRs target it or the layer below. Only the final phase PR targets
  `develop`.
- `feature/` is not used for sprint work.
- The worktree directory equals the branch name.
- Phases before a naming rule keep their existing names. Do not rename merged
  history.

## Code Samples

Important traits, enums, protocol types, interfaces, and boundary contracts
must have explicit code samples or signatures when prose alone would leave
implementation choices open. In a phase with a contract sprint they live in
that sprint bead's design; layer sprints reference them and do not restate them.

## Recommended Agent / Model

Each sprint bead names its agent as its assignee (the ATM identity) and its
model in `model_class` metadata, chosen by the tier the work needs: a fast agent for
bounded or documentation work; the workhorse for typical work; a
deep-reasoning agent for algorithmic, architectural, or performance work.
When the repository keeps a developer roster (its path is in
`.claude/project/quality-policy.md`), name the agent from it, preferring
a named team member over a background agent. How many agents run is decided
at dispatch, not in the plan. Layer sprints are bounded by construction
and suit one developer and one QA pass each, running concurrently.

## QA Consumption

Sprint beads must be short and structured enough that:

- `req-qa` can enumerate the `requirements` list, deliverables and
  acceptance criteria directly, and rejects a sprint bead whose REQ/NFR list
  is missing or incomplete
- `arch-qa` can enumerate the `adrs` list and structural gate artifacts
  directly, and rejects a sprint bead whose ADR list is missing or incomplete
- `ruthless-boundary-qa` can check `owned_paths` and `target_boundary`
  against the repository's declared boundary manifests and validation policy
- `quality-mgr` can route QA without copying scope by hand

QA scope follows the closure type: a `boundary` sprint is reviewed against
one crate, one manifest, and its boundary-rooted criteria. Full-stack review
belongs to the integration sprint and the phase-ending review. If a sprint bead
cannot be reviewed that way, shorten or tighten it instead of adding more
prompt ceremony.

## Finding Classification

Classify each finding as either structural or wording before assigning
severity.

Structural findings:
- missing acceptance or validation gate
- incorrect command, test name, or grep gate
- uncovered call site, file, module, or runtime path
- missing type, trait, function, boundary contract, or ADR
- a sprint bead with a missing or incomplete `requirements` or `adrs` list, a
  deliverable that traces to no REQ/NFR, or plan content that contradicts a
  REQ, NFR or accepted ADR
- a multi-boundary sprint without `vertical_rationale`, overlapping
  `owned_paths`, or a `must_follow` edge with no named contract artifact
- a thin sprint, a layer cut that creates no parallel work, or a single
  phase-wide integration checkpoint
- a sprint id, label or branch name that breaks "Naming"
- phase-level false closure: feature behaviour no integration sprint owns

Structural findings always remain in the main `findings` array and must be
rated `Blocking` or `Important` when they affect implementability, closure,
or the plan's ability to run in parallel.

A "missing gate" finding is structural only when a deliverable's behavior
would otherwise go unverified. A finding whose only remedy is a new process
artifact must pass the Process Artifacts rule above; otherwise it is debt
notes, not a finding. Over-specification (unjustified artifacts, restated
contracts, redundant inventories) is itself a valid finding.

Wording findings:
- prose ambiguity that does not change scope or closure meaning
- formatting cleanup
- non-normative wording polish

Wording findings belong in `minor_wording` and do not fail the round unless
the reviewer marks them `affects_ac: true`.
