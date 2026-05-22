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
- newly discovered issues found during `Z.4` that are out of scope for the
  frozen `Z.3` handoff must be recorded here using `status: out_of_scope`
  rather than fixed in the `Z.4` sprint
