# Phase Yb QA Handoff

## Purpose

Provide one QA-owned checklist for the Yb implementation line so `req-qa` and
`arch-qa` verify payload contracts, removal targets, and boundary enforcement
mechanically rather than by prose summary.

## Authoritative Inputs

QA for every Yb sprint must treat these as authoritative:

- `docs/phase-Yb/plan-phase-Yb.md`
- the active Yb sprint doc
- `docs/phase-Yb/removal-ledger.md`
- `docs/phase-Yb/message-path-call-stacks.md`
- `docs/phase-Yb/lintable-boundary-plan.md`
- `docs/phase-Yb/testing-and-validation.md`
- `docs/adr/ADR-013-unified-delivery-plan-and-state-machine-ownership.md`

## Required QA Coverage

### 1. Delivery-plan contract

`req-qa` must verify:

- `DeliveryPlan` and `ReplyDeliveryPlan` are present in
  `atm_core::delivery_plan`
- Claude and non-Claude paths emit the same logical `message[]` payload set
- SQLite-failure branches emit:
  - `message[1] = original message`
  - `message[2] = atm-system@<team>` error message
- post-send notification evidence is never accepted as delivery proof

### 2. Removal-ledger closure

For any Yb sprint that closes deletion targets, `req-qa` must search the whole
workspace for every construct family named in the sprint doc and in
`removal-ledger.md`.

`arch-qa` must verify:

- the replacement path exists
- the old call stack is no longer authoritative
- no hidden second caller class remains

### 3. Boundary enforcement

QA must verify that only approved callers can reach:

- `InboxExport` append/rewrite primitives
- `NonClaudeOutbound` payload delivery
- `NotificationSink` notification execution

Required evidence:

- boundary TOML files list the owner and allowed dependents
- `python3 .just/run_lint.py all` passes
- grep checks in the sprint doc show no forbidden direct callers

### 4. Fault-injection and degraded paths

For Y.7 and later, QA must verify named tests or test families for:

- SQLite success + Claude success
- SQLite success + Claude append degradation
- SQLite failure + Claude path
- SQLite failure + non-Claude path
- companion-error failure

### 5. Phase-end closure

Y.10 is not complete unless QA can prove:

- all removal-ledger items are closed or explicitly tracked as blockers
- only approved executor/coordinator modules can call the low-level delivery
  primitives
- smoke/dogfood planning can resume without reopening Yb path-consolidation
  work
