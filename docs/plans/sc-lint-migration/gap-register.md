---
title: sc-lint Migration Gap Register
status: template
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
---

# sc-lint Migration Gap Register

This file is created during planning so the execution branch has one
authoritative place to track missing released capability, temporary ATM
workarounds, and deletion triggers.

## Pinned Release

- released `sc-lint` version: `TBD`
- install method:
  - Linux: `TBD`
  - macOS: `TBD`
  - Windows: `TBD`

## Gap Table Schema

Every row must use this schema:

| Gap ID | Surface | Classification | Current status | Upstream owner | ATM workaround allowed | Deletion trigger |
| --- | --- | --- | --- | --- | --- | --- |

Classification must be exactly one of:

- `atm-wiring-bug`
- `sc-lint-product-gap`
- `atm-consumer-specific`

Current status must be exactly one of:

- `known-before-integration`
- `discovered-during-integration`
- `closed-upstream`
- `closed-in-atm`

## Initial Known Gaps To Review

| Gap ID | Surface | Classification | Current status | Upstream owner | ATM workaround allowed | Deletion trigger |
| --- | --- | --- | --- | --- | --- | --- |
| `SCLINT-GAP-001` | `unix_path_prefixes` portability-config parity | `sc-lint-product-gap` | `known-before-integration` | `sc-lint team` | only the narrowest wrapper/config shim required to preserve `unix-gating` behavior | published `sc-lint-portability` exposes equivalent config or ATM proves the behavior is no longer needed |
| `SCLINT-GAP-002` | JSON output / machine-contract parity for ATM lint surfaces | `sc-lint-product-gap` | `known-before-integration` | `sc-lint team` | only a compatibility adapter that preserves ATM lint parsing/report shape | released `sc-lint` exposes the needed machine contract directly |
| `SCLINT-GAP-003` | rule-ID continuity for `PORT-004`, `PORT-005`, `SCB-RUNTIME-001`, `SCB-RUNTIME-002` | `sc-lint-product-gap` | `known-before-integration` | `sc-lint team` | only the minimum ATM-local mapping/shim needed during migration | released rule IDs and selection surfaces are proven stable for ATM |
| `SCLINT-GAP-004` | all-platform installation coverage, especially Windows | `sc-lint-product-gap` | `known-before-integration` | `sc-lint team` | no workaround beyond one documented install method chosen in Sprint 01 | one supported published install path is proven on Linux, macOS, and Windows |
