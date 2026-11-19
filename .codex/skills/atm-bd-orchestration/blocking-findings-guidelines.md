# Blocking findings and scoped decisions

The lead scopes decisions as well as work. A reviewer calling a finding
blocking does not establish that the whole phase must stop. Escalating a
question to the user is appropriate; waiting for an answer must not idle
independent work.

## Check scope before priority

First determine whether the finding belongs to the approved sprint and phase.
Check its governing requirement, affected code and assigned ownership; a
severity label or a discovery during QA does not add work to the plan. A
valid issue outside the phase goes to the appropriate backlog/owner with its
evidence, rather than becoming a prerequisite for unrelated phase work.

For example, changing an sc-lint pin to an unreleased version is not part of
a logging phase merely because a validation run exposes it. Do not make that
change or expand the phase silently. If an external issue demonstrably prevents
the planned feature from working, record that specific dependency and continue
unaffected work while its owner handles it.

## Check the finding against the contract

Read the cited code at the reported commit and the current integration or
fix head. Consult the governing requirements, ADRs, approved interfaces and
the bead's ownership boundaries. Separate the observed defect from the
reviewer's proposed remedy: a valid defect can have an overbroad remedy.

Do not invent an API or broaden a sprint merely to satisfy a mistaken test
expectation. Record a contract-based correction on the finding, preserving
its original evidence and rationale. Correcting the remedy does not prove
the fix complete: its actual scope still needs the required sanity and QA.
Do not call missing behavior implemented or waive an approved requirement.

## Reject unnecessary process remedies

Apply `/just-say-no-to-process-porn-and-ceremony` when a finding proposes
process artifacts or when activity is producing bookkeeping instead of
working capability. A certificate, ledger, matrix, report or new check must
have a concrete consumer, a gate it enforces, an observed defect it addresses,
and a retirement condition. An explicit user request supplies a valid consumer
and purpose; it is not a license to add more machinery around that request.

Preserve a real defect while rejecting an unnecessary process remedy. Use the
existing bead and evidence instead of building a parallel tracking system.
Do not weaken tests, manufacture PASS evidence, or move unfinished acceptance
criteria into follow-ups merely to close the original item. Process changes
and decision records do not count as delivered feature capability.

## Record and scope the decision

Create a `decision` bead in the phase and assign it to the user or
their delegate, as appropriate. Reuse that record as evidence develops. Include:

- The question and concrete options, including which is conservative and why.
- The governing contract and affected beads, files, interfaces and consumers.
- The impact of choosing each option incorrectly: correctness, compatibility,
  performance, rework and how easily the choice can be reversed.
- The provisional choice, its rationale, and any work that actually needs
  the final answer. Distinguish lead action from owner approval.

The unresolved decision must prevent phase closure. A child decision bead
provides that closure obligation; it must not acquire development-blocking
edges merely because its owner has not answered. Record the final disposition
and verify any resulting work before closing the decision. An unanswered
question or elapsed time is not approval.

## Keep development moving conservatively

For low-impact, reversible choices, the lead selects a provisional option
consistent with the architecture: avoid overcomplication, preserve modular
boundaries, keep performance high, and keep the implementation clean. Notify
the decision owner asynchronously and continue the team’s independent work.
Do not make radical or costly-to-reverse changes while awaiting the answer.

For example, when deciding between a new crate and an existing crate, use the
existing crate provisionally if its ownership and dependency boundaries permit
it. Keep the code separable so extraction remains practical. Do not introduce
a new crate or a forbidden dependency simply to resolve the question quickly.

A significant architectural decision may justify holding the work that
depends on it. Identify that dependency explicitly and continue unaffected
work. A project-wide hold requires evidence that the architectural issue
affects the whole project; finding severity, a shared wave label, or a pending
reply alone is insufficient.

Use the approved bead dependencies and the normal stack, readiness and
verification workflow. Do not ignore a real dependency or silently rewrite
the planned sprint DAG to get work dispatched. Where a finding gate holds
independent work, document the scoped scheduling correction under the user's
authority, preserve the finding, and validate the graph. Never close an
unresolved finding or claim a PASS merely to release a queue.

These guidelines were requested by the user after a mis-scoped test finding
stopped independent phase work. Their consumer is the orchestration lead;
they govern escalation and development holds. Retire or replace this guidance
when that decision policy is superseded, rather than growing a second workflow.
