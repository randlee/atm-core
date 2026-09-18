# Sprint Planning Guidelines

Use these rules when writing and hardening sprint plans.

The unit of a sprint is a **boundary**, not a feature. Crate boundaries are
trait-bound contracts recorded in `boundaries/<crate>/*.toml` and enforced by
`just lint`; they are the unit of independence this architecture already
checks mechanically. A plan that cuts sprints along them can run its sprints
at the same time. A plan that cuts sprints by feature makes every sprint touch
the same crate stack and forces a serial chain. Ten non-intersecting sprints
in parallel are preferred over five in series.

## Core Rules

- The sprint plan is authoritative.
- Downstream prompts may carry structured projections of sprint-plan data, but
  they must not replace or narrow the sprint plan.
- If QA cannot review directly from the sprint doc, the sprint doc is not
  hardened.

## Plan From The Boundary Map

Before cutting any sprint, the phase plan records a **boundary map**: every
crate and boundary manifest the phase changes, the contract change at each
(trait methods, types, schema, wire fields, error codes), and the
`allowed_dependents` / `allowed_dependencies` edges between them, read from
the manifests. Sprints are cut from this map, never from the feature list.

A phase has three kinds of sprint, in this order:

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
3. **Integration sprints** (final wave). Composition-root wiring, end-to-end
   and CLI behaviour, colima/smoke procedures, and user-facing docs. Every
   feature-level acceptance criterion of the phase lives here, exactly once.

Features do not get sprints. A feature closes at the integration sprint, after
the layer sprints in its dependency chain have closed.

## Closure Types

Every sprint doc declares one `closure_type` and one `target_boundary`.

| `closure_type` | Closes when | Required validation |
|---|---|---|
| `contract` | every interface the phase changes is committed as signatures, types, manifests and ADR text; test doubles and contract tests compile; workspace builds | `cargo build --workspace`, `just lint` |
| `boundary` | the sprint's side of the contract is fully implemented in its one crate; contract tests pass against it (implementer) or its tests pass against the test double (consumer); boundary lint is clean; no `todo!`, `unimplemented!` or stubbed branch remains | `cargo test -p <crate>`, `cargo clippy -p <crate> -- -D warnings`, `cargo build --workspace`, `just lint` |
| `integration` | every feature-level acceptance criterion of the phase passes through the real composition; the phase leaves no contract without a production consumer | full workspace validation plus the phase's end-to-end procedures |
| `docs` | the named documents match the shipped surface | doc lint |

A `boundary` sprint does not claim, test, or demonstrate behaviour that
passes through another crate. It lists that behaviour under "This Sprint Does
Not Close" and names the integration sprint that closes it.

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

Each sprint doc lists `owned_paths` (globs). Within a wave no path may match
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
- `must_follow` merge-forward trigger: parent development is pushed, not QA;
  merge parent → child before every dev/fix round. PR-completion trigger:
  parent PR merges first.

The phase plan publishes a **wave table**: each wave, its sprints, their
`target_boundary` and `owned_paths`, plus two numbers: **critical path**
(longest `must_follow` chain, in sprints) and **width** (most sprints in one
wave). The expected shape is three waves: contract, layers, integration.
Every `must_follow` edge that lengthens the critical path past three needs a
recorded reason a reviewer can check.

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

## Sprint Doc Shape

Each sprint doc should have one authoritative list for:

- deliverables
- acceptance criteria, each with its root
- owned paths
- paths to delete, when applicable
- required validation

Do not restate the same checklist item in multiple sections with different
wording.

## Code Samples

Important traits, enums, protocol types, interfaces, and boundary contracts
must have explicit code samples or signatures when prose alone would leave
implementation choices open. In a phase with a contract sprint they live in
that sprint's doc; layer sprints reference them and do not restate them.

## Recommended Agent / Model

Optional `recommended_agent`/`recommended_model` select from the current
developer pool: Cipher-311d/fast for bounded or documentation work;
arch-ctm/deep-reasoning for algorithmic, architectural, or performance work.
They are advice, not an assignment. Layer sprints are bounded by construction
and suit one developer and one QA pass each, running concurrently.

## QA Consumption

Sprint docs must be short and structured enough that:

- `req-qa` can enumerate deliverables and acceptance criteria directly
- `arch-qa` can identify structural gate artifacts directly
- `quality-mgr` can route QA without copying scope by hand

QA scope follows the closure type: a `boundary` sprint is reviewed against
one crate, one manifest, and its boundary-rooted criteria. Full-stack review
belongs to the integration sprint and the phase-ending review. If a sprint doc
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
- a multi-boundary sprint without `vertical_rationale`, overlapping
  `owned_paths`, or a `must_follow` edge with no named contract artifact
- phase-level false closure: feature behaviour no integration sprint owns

Structural findings always remain in the main `findings` array and must be
rated `Blocking` or `Important` when they affect implementability, closure,
or the plan's ability to run in parallel.

Wording findings:
- prose ambiguity that does not change scope or closure meaning
- formatting cleanup
- non-normative wording polish

Wording findings belong in `minor_wording` and do not fail the round unless
the reviewer marks them `affects_ac: true`.
