---
title: AL.9 Physical Proof, Performance Gate, and Removal-Ledger Freeze
status: active
branch: feature/pal-s9-physical-proof-ledger-freeze
worktree: ../atm-core-worktrees/feature/pal-s9-physical-proof-ledger-freeze
target: integrate/phase-al
---

# AL.9 — Physical Proof, Performance Gate, and Removal-Ledger Freeze

**recommended_agent:** arch-ctm/deep-reasoning. Team-lead assigns the named M5
proof operator at sprint dispatch and records that identity in the artifact;
team-lead is the cutover-switch authority and names the release operator in
the cutover table before activation.
**must_follow:** AL.8 and AL.4's accepted graft outbound-client migration.
Merge both pushed integration commits before every development/fix round; their
PR merges are not required. AM deletion is a PR-completion gate: it cannot
start until AL.9's evidence and ledger input are accepted.
**unblocks:** AM.1 ledger freeze and AM.2–AM.6 deletion work after acceptance.
**parallel_safe:** none for activation or ledger freeze.

**traceability:** `REQ-CORE-TRANSPORT-001/001B/002/004/005`,
`REQ-DAEMON-TRANSPORT-001/003/005/008`, `REQ-P-RUNTIME-001`–`006`, ADR-032,
ADR-033, ADR-036, and the AL/AM boundary checklist.

## Deliverables

1. Execute one physical proof matrix against the AL.8 composition: in-process,
   Unix UDS, loopback TCP, `atm-graft` write, and M5
   direct cross-host write. Every case records the one route, shared client,
   `ApiRouter` dispatch, storage boundary, and receive-hook call path.
2. TLS is out of MVP scope. PR #774 (`0c3bc49a`) quarantined
   `atm-peer-tls-interop` as reference material and removed the legacy HTTPS
   transport from this line; AL.7 was never implemented. AL.9 must neither
   build nor activate a TLS adapter, and must not claim an AL.7 artifact reuse.
3. Complete the non-TLS AL.7 closure item that was skipped with that sprint:
   migrate CLI and graft write calls to the existing shared UDS-preferred /
   loopback-TCP `DaemonApiClient` implementation and retire their write-path
   callers. `atm_daemon_client::{exchange_request, try_connect}` and the
   compatibility wrapper remain live only for synchronous CLI/graft
   read/ack/admin dispatch; AL.9 records their async conversion and deletion
   as explicit AM.1 ledger work. No new shim, TODO, retry, or replay path may
   be added. This remediation is client-side only; the frozen legacy
   `atm-daemon` crate remains untouched.
4. Compare the baseline captured at `develop` `67401907` before AL.1 against
   the AL runtime with a fixed workload and declared p50/p99 latency,
   throughput, tolerance, hardware, OS, toolchain, and raw artifacts. Measure
   hook-disabled and hook-active latency. Include an actual Windows CI or
   measurement result; no "verified equivalent" substitutes for it.
5. Publish the adapter cutover table: add, activate, retire, owner, rollback,
   and endpoint-record publisher for each physical adapter. During transition,
   exactly one active listener and one endpoint-record publisher exist per
   endpoint. The team-lead-authorized release operator performs the hard
   switch; that operator is recorded in the artifact before activation.
6. Capture AL.8's actual live-reference graph. AM.1 then freezes its draft
   ledger against that graph, including the disposition of observability,
   doctor, dashboard, and configuration consumers. Numeric AM sprint labels
   never override this topology.
7. Schedule and record the M5 window before the proof starts. If M5 proof,
   benchmark tolerance, or any cutover invariant fails, retain legacy as the
   active path, park the AL integration line, do not start AM, and do not
   freeze the ledger. A new approved proof round is required to resume.

## Final `integrate/phase-al` to `develop` evidence pass

The remaining physical rows are not parked to AM and are not satisfied by a
source review or historical artifact. They are a bounded final AL gate.

- **Owner:** `team-lead` is the evidence-pass coordinator and release-decision
  owner. The M5 execution operator is `arch-ctm` on M5; team-lead names the
  Windows execution operator for the frozen candidate.
- **Trigger:** start after every code and documentation PR intended for the
  release candidate is merged and quality-approved on `integrate/phase-al`,
  its exact SHA is frozen, and before that SHA is proposed for `develop`.
- **Required rows:** physical M5 cross-host write; physical Windows TCP
  benchmark; CLI write/activation; graft write/nudge/read/ack; and the one
  active `atm-http-runtime` listener plus one endpoint-publisher check.
- **Record:** retain self-contained `site/reports/` artifacts, the candidate
  SHA, platform and host metadata, raw benchmark samples where applicable,
  `atm doctor --json` before/after evidence, and a pass/fail matrix decision.

This is the only authorized backfill window for these rows. A failed or
missing row blocks the `develop` merge and keeps AM's deletion work parked; it
must not be substituted with a loopback alias, static proof, or a pre-Tokio
artifact.

## Acceptance criteria

- The matrix proves one active client/route/handler path for every adapter,
  including graft; direct failure creates no retry/replay work.
- The M5 direct-cross-host artifact is captured for this AL.9 proof revision;
  no TLS adapter or TLS-reuse artifact is in scope.
- CLI and graft writes use the one shared UDS/loopback `DaemonApiClient` path;
  synchronous legacy client symbols are proven absent from write call chains
  and recorded as explicit AM.1 work for retained read/ack/admin dispatch.
- The benchmark gate has raw baseline and result artifacts with its stated
  tolerances, including Windows and hook-active measurement.
- The cutover table proves one listener/publisher per endpoint and defines a
  safe rollback/park result.
- AM.1's ledger input is frozen from the actual reference graph, not stale
  documentation; AM deletion has not begun.

## Required validation

- full test, format, lint, dependency/boundary checks
- physical proof artifacts for every matrix row
- every live smoke artifact records platform and host and is retained as one
  self-contained directory beneath `site/reports/smoke/`; no shared/latest
  report may overwrite a concurrent hardware run
- M5 clean-checkout artifact or approved SHA-reuse justification
- baseline/result benchmark artifacts and Windows result
- independent review of cutover table, reference graph, and ledger freeze

## Non-closure

AL.9 does not delete legacy source, begin recovery/replay, or change any public
transport struct/serialization. It is the evidence gate for Phase AM only.
