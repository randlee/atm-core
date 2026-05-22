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
- `status`
- `z4_disposition`
- `revalidation_result`
- `notes`

## Rules

- only validated `Z.3` operator findings may enter this ledger
- `Z.4` fixes only findings recorded here
- deferred items must record explicit approval in the ledger notes
