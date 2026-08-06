# AL.3 Critical Review Task List

Review baseline: `c9097e03` (`feature/pal-s3-received-hook`). This is a
post-implementation review against `sprint-AL3-received-hook.md`; each item
must be closed with executable evidence before the follow-up commit is sent
for QA.

- [x] **AL3-CR-001 — explicit newly-persisted disposition.** Replaced the
  dispatcher’s generic post-write predicate with a named persistence-result
  predicate. It must make the new-versus-idempotent distinction visible at the
  one hook-routing decision and have direct tests for new and duplicate cases.
- [x] **AL3-CR-002 — behavioral ingress convergence proof.** Added a
  deterministic test seam proving that local UDS, local TCP, and peer HTTPS
  provenance converge through the same post-persistence router decision;
  retain the existing architecture guard as a defense-in-depth check.
- [x] **AL3-CR-003 — deadline warning contract.** Added a deterministic test
  for an exhausted inherited hook budget after persistence: the hook must not
  create detached work and the successful durable-write outcome must retain a
  warning when it can be serialized within the request contract. Document the
  existing outer request-deadline behavior separately rather than hiding it.
- [x] **AL3-CR-004 — re-run scope and boundary validation.** Targeted tests,
  tests plus `just test` and `just lint all`; confirm no daemon dependency on
  `atm-graft`, no client-side hook call, and no reintroduced notification
  queue/retry worker.
- [x] **AL3-CR-005 — router-boundary terminology.** Reconciled the
  `PostWriteRouter` contract with its retained temporary peer-delivery wake-up
  responsibility, while keeping the receiver hook exclusive to newly
  persisted inbound writes.
