# Hermes ATM Native Tools — Worktree Checklist

Scope: `atm_send`, read-only `atm_read`, and `atm_list` in `hermes-atm`.
Execution rule: complete and verify each item before marking it done. Do not
perform live gateway-profile testing until every offline item and the second-pass review
are complete.

- [x] 1. Reconcile package boundary: add `pydantic` to `atm-graft-python`,
  update its boundary contract, and preserve `hermes-atm` as a thin consumer.
- [x] 2. Complete typed graft list support: `ListQuery` → `ListOutcome` through
  `AtmGraftClient`, `GraftClient`, and Python `PyGraftSession`; test it.
- [x] 3. Define strict Pydantic ingress models for send, read-only read, and
  list; reject unknown fields and mutation inputs before native transport.
- [x] 4. Implement structured, JSON-safe success/error adapters. Validate
  ingress only; serialize trusted typed outcomes directly.
- [x] 5. Register the three tools through Hermes's public plugin seam with
  package-owned, idempotent installation and fail-closed capability checks.
- [x] 6. Add isolated tests for schemas, typed translations, error layering,
  result shape, registration idempotence, and prohibited/mutating inputs.
- [x] 7. Run focused Python/Rust tests, package/wheel discovery tests, and the
  repository lint/test suite; fix all task-local failures.
- [x] 8. Second pass: re-read this checklist, requirements/ADR, boundary
  policy, and implementation; review crate/Python boundaries, error recovery,
  ownership, and test coverage for best-practice compliance.
- [x] 9. Write a low-risk installed-package proof plan: package-only install, managed
  gateway reset, one distinct tool invocation per operation, observation,
  and rollback criteria. Do not execute it without clean offline validation.
