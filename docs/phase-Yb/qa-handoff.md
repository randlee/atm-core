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

Final Yb closeout through Y.11 is not complete unless QA can prove:

- all removal-ledger items are closed or explicitly tracked as blockers
- only approved executor/coordinator modules can call the low-level delivery
  primitives
- smoke/dogfood planning can resume without reopening Yb path-consolidation
  work
- normal Claude append delivery fails closed on JSON-array inbox files instead of
  silently rewriting them through the rebuild seam
- rebuild/reexport rewrite remains available only through the explicit
  repair/rebuild boundary path
- the low-level Claude append seam is never selected for
  `DeliveryHarnessPath::NonClaude`
- the repair/rebuild refresh seam is explicit by construction and not a generic
  recipient-routed helper that silently ignores non-Claude requests

#### Evidence mapping

| Closure condition | Satisfying artifact |
| --- | --- |
| All removal-ledger items closed or tracked | `docs/phase-Yb/removal-ledger.md` — every row has a `closed-by` sprint entry or an explicit `BLOCKER` annotation |
| Only approved executors call low-level delivery primitives | `docs/phase-Yb/lintable-boundary-plan.md` caller allowlist table; `python3 .just/run_lint.py all` passes |
| Smoke/dogfood planning can resume | Sprint Y.11 closeout sign-off in `docs/phase-Yb/sprint-Y11.md` §Completion |
| Claude append fails closed on JSON-array inbox files | `service_runtime::tests::append_compat_inbox_message_rejects_legacy_array_mailbox_from_runtime_path` |
| Rebuild/reexport only through explicit repair/rebuild path | `service_runtime::tests::rebuild_compat_inbox_projection_reexports_store_backed_mailbox`; lintable-boundary-plan allowlist row for `mailbox::store::write_compat_mailbox_projection` |
| Claude append seam not selected for `DeliveryHarnessPath::NonClaude` | `delivery_execution` runtime fail-closed checks; lintable-boundary-plan §3 rule 6 |
| Repair/rebuild seam is explicit, not a generic helper | `RetainedServiceRuntime::rebuild_compat_inbox_projection` scoped as repair-only seam; lintable-boundary-plan §2 rule 3 |
