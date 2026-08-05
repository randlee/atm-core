---
title: AK.17 Optional heartbeat replay physical conformance and performance proof
status: deferred_pending_AK15_acceptance
branch: feature/pak-s17-heartbeat-replay-conformance
worktree: ../atm-core-worktrees/feature/pak-s17-heartbeat-replay-conformance
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.15 merged to integrate/phase-ak
merge_gate: AK.15 PR merge and optional-replay implementation QA pass
parallel_safe: false
quality_findings: []
---

# AK.17 — optional heartbeat replay physical conformance and performance proof

## Closure

AK.17 is the sole physical-evidence owner for the optional AK.15 replay
extension. It runs only if AK.15 was explicitly approved and merged. It does
not change runtime behavior: any failure is a new finding against AK.15, not a
test-only compatibility workaround.

## Authoritative deliverables

1. A tracked, sanitized physical evidence bundle at
   `artifacts/phase-ak/AK17-heartbeat-replay-conformance/`, including the
   merged AK.15 SHA, binary versions, peer identities, setting state,
   heartbeat timeline, ordered page IDs, cursor observations, receiver-hook
   observations, and normalized command transcript.
2. M4↔M5 and M4↔Windows physical results for both disabled and enabled
   `peer_heartbeat_replay` modes.
3. Before/after direct-singleton latency/allocation benchmark results, with
   the exact command, baseline SHA, AK.15 SHA, environment, raw normalized
   results, and comparison.
4. An update to `docs/peer-pair-smoke.md` that distinguishes the minimal
   AK.13 no-replay procedure from the optional enabled-replay procedure.
5. A recorded deterministic redaction check before the evidence bundle is
   tracked: use the repository-approved secret scanner when available, or the
   fixture-tested script established by AK.13. Record its exact command and
   result in the manifest; credentials, private keys, bearer tokens, and
   unredacted configured secrets are rejected.

## Paths to delete

None. This is an evidence-only sprint. It must not alter production source or
weaken an AK.12 guard.

## Acceptance criteria

- Disabled mode reproduces AK.13: outage then restoration causes no replay.
- Enabled mode observes one unavailable→healthy heartbeat transition, sends
  exactly one bounded oldest page on that transition, confirms the exact page
  once, and emits no sender-side hook.
- A remaining page drains only on a later healthy heartbeat; a failed page
  remains unconfirmed and is not retried until another observed
  unavailable→healthy transition.
- Receiver results use the same canonical route/admission path, produce one
  hook per newly persisted member, suppress duplicate hooks, and return hook
  failures only as warnings.
- The benchmark shows the default-off direct singleton path has no material
  regression against the recorded AK.13 baseline. Enabled replay is reported
  separately and never measured as part of the direct fast path.
- The Windows lane is required for optional-replay closure. If it is
  unavailable, AK.17 remains blocked; no QA or plan-hardening sprint may
  convert the lane into an accepted exception without explicit operator
  waiver recorded in the evidence bundle.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  on the exact merged AK.15 baseline before each physical run.
- Independent audit of the tracked evidence bundle; ignored smoke logs and a
  verbal result are insufficient.
- Re-run the AK.12 mechanical guards unchanged before recording evidence.

## Explicit prohibitions

- No code, configuration-default, endpoint, decoder, route, persistence, or
  hook behavior change to make a physical lane pass.
- No local-only, mock-only, curl-only, or manually reissued-send substitute
  for an enabled heartbeat replay observation.
- No attempt to close AK.17 when a required physical platform is unavailable.

## Dependencies and handoff

AK.17 follows AK.15 PR completion and is not parallel-safe with it because it
measures the exact merged runtime behavior. If AK.15 is never approved, AK.17
remains deferred and has no effect on minimal Phase-AK closure. If it runs,
AK.16 follows it to harden the complete plan set before optional-replay phase
closure.
