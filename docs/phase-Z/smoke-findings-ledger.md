# Phase Z Smoke Findings Ledger

## Purpose

Authoritative findings ledger for `Z.1` smoke results and `Z.2` revalidation.

## Record Schema

Each finding entry must record:

- `finding_id`
- `discovered_in`
- `linked_flow_id`
- `summary`
- `severity`
- `fix_owner`
- `status`
- `z2_disposition`
- `revalidation_result`
- `notes`

## Rules

- only verified `Z.1` findings may appear in this ledger
- `Z.2` fixes only findings recorded here
- if a `Z.1` observation is rejected as non-reproducible, that outcome must
  still be recorded here rather than dropped silently
