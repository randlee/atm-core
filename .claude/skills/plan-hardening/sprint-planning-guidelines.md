# Sprint Planning Guidelines

Use these rules when hardening sprint plans.

## Core Rules

- The sprint plan is authoritative.
- Downstream prompts may carry structured projections of sprint-plan data, but
  they must not replace or narrow the sprint plan.
- If QA cannot review directly from the sprint doc, the sprint doc is not
  hardened.

## Split Early

Split a sprint immediately when any of these are true:

- there is credible doubt that every committed deliverable can land at a
  production-ready level in the same sprint
- the sprint mixes too many closure types
- the sprint touches too many modules, boundaries, or runtime paths for clear
  ownership
- acceptance criteria would allow one deliverable to slip while the sprint
  still claims success
- the same deliverable is being planned more than once across multiple sprints

Do not preserve an overloaded sprint just to keep the sprint count low.

## Sprint Doc Shape

Each sprint doc should have one authoritative list for:

- deliverables
- acceptance criteria
- paths to delete, when applicable
- required validation

Do not restate the same checklist item in multiple sections with different
wording.

## Production-Ready Expectation

Every listed deliverable must be expected to land at a production-ready level
for the scope that sprint claims.

Do not allow:

- shape-only completion
- test-only completion
- boundary-only completion when runtime behavior is still open
- silent carry-forward of a committed deliverable

If a sprint intentionally does not close something, state that explicitly under
non-closure or out-of-scope sections.

## Code Samples

Important traits, enums, protocol types, interfaces, and boundary contracts
must have explicit code samples or signatures in the sprint doc when prose
alone would leave implementation choices open.

## QA Consumption

Sprint docs must be short and structured enough that:

- `req-qa` can enumerate deliverables and acceptance criteria directly
- `arch-qa` can identify structural gate artifacts directly
- `quality-mgr` can route QA without copying scope by hand

If that is not true, shorten or tighten the sprint doc instead of adding more
prompt ceremony.

## Finding Classification

Classify each finding as either structural or wording before assigning
severity.

Structural findings:
- missing acceptance or validation gate
- incorrect command, test name, or grep gate
- uncovered call site, file, module, or runtime path
- missing type, trait, function, boundary contract, or ADR
- false-closure wording that hides still-open runtime or boundary work

Structural findings always remain in the main `findings` array and must be
rated `Blocking` or `Important` when they affect implementability or closure.

Wording findings:
- prose ambiguity that does not change scope or closure meaning
- formatting cleanup
- non-normative wording polish

Wording findings belong in `minor_wording` and do not fail the round unless
the reviewer marks them `affects_ac: true`.
