# Phase Z Canary Findings Ledger

## Purpose

Authoritative findings ledger for `Z.3` and the only finding source for `Z.4`.

## Record Schema

Each finding entry must record:

- `finding_id`
- `participant`
- `linked_operator_flow`
- `summary`
- `severity`
- `fix_owner`
- `status`
- `z4_disposition`
- `revalidation_result`
- `notes`

## Rules

- only validated `Z.3` operator findings may enter this ledger
- `Z.4` fixes only findings recorded here
- deferred items must record explicit approval in the ledger notes
- every row must record one final `z4_disposition` before the release verdict
  in `docs/phase-Z/readiness.md` may leave `PENDING`
- newly discovered issues found during `Z.4` that are out of scope for the
  frozen `Z.3` handoff must be recorded here using `status: out_of_scope`
  rather than fixed in the `Z.4` sprint

## Validated Findings

No validated `atm-dev` canary findings were promoted during `Z.17`.

| finding_id | participant | linked_operator_flow | summary | severity | fix_owner | status | z4_disposition | revalidation_result | notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `Z4-OOS-001` | `arch-ctm` | `just smoke normal` retained-log severity gate | expected invalid-ack recovery originally emitted `ATM_MESSAGE_VALIDATION_FAILED` into the normal-lane severity gate and failed `FAST-LOG-002` even though the normal lane otherwise passed | `blocking` | `arch-ctm` | `resolved` | `fixed_in_z4` | `PASS` | fixed on `feature/pZ-smoke-atm-graft @ 84935774`; the analyzer now allows the one expected `ATM_MESSAGE_VALIDATION_FAILED` record in the normal validation/recovery contract while keeping fast/thorough healthy-path severity gates strict; artifact: `reports/smoke/smoke.md` |
