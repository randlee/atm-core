---
title: AK.11 Independent M5 disabled-cache cross-host proof
status: proposed
branch: feature/pak-s11-m5-crosshost-proof
worktree: ../atm-core-worktrees/feature/pak-s11-m5-crosshost-proof
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.6 merged to integrate/phase-ak
merge_gate: AK.6 merge commit
parallel_safe: false
quality_findings: [AK5-CROSSHOST-PROOF-001]
---

# AK.11 — independent M5 disabled-cache cross-host proof

## Closure

Close `AK5-CROSSHOST-PROOF-001` only with a physical, bidirectional M5 proof
run from the accepted `integrate/phase-ak` line **after AK.6 merges**. This is
an independent sprint: it is not an AK.6 smoke subtask, a localhost result, a
unit mock, or a claim based on ignored `reports/smoke` output.

The receiver remains the one canonical inbound path for CLI, graft, and
cross-host HTTP. It accepts the AK.8 `messages[]` envelope, durably admits the
entire accepted request, then attempts the ordinary post-commit nudge
best-effort. A nudge error is diagnostic evidence only; it cannot make the
receive fail. The sender either posts the immediate singleton when healthy or
one recovered ordered array after a healthy transition. One whole-array success
advances the durable outbound cursor once.

## Physical proof contract

1. Pin the exact post-AK.6 `integrate/phase-ak` SHA and record both M4 and M5
   ATM binary versions, host identities, peer configuration fingerprints, run
   ID, and timestamps. Do not record credentials or private key material.
2. First set `peer_resend_cache = false` on both physical peers and prove it
   from the effective daemon configuration. Run M4→M5 and M5→M4
   `crosshost-send`, `crosshost-ack`, and `crosshost-curl-plain` lanes. Each
   production-send case proves exact remote ULID/body, host-qualified
   rendering, one ordinary receiver nudge, and no sender-side cross-host nudge.
3. While cache is disabled, induce one controlled transport failure in each
   direction. Prove the send returns the persisted-but-unconfirmed typed
   failure, leaves the origin `peerOutbound` marker durable, and makes no
   automatic resend through at least one complete configured due window. The
   receiver must have no false delivery/nudge for the failed request.
4. Only after the disabled-cache proof succeeds, enable the cache and run a
   separate recovery proof: the retained ordered backlog is delivered as one
   array request on a healthy transition and one success atomically retires the
   full submitted marker set. This must not be presented as disabled-cache
   behavior.
5. Retain a sanitized, reviewable evidence bundle under a non-ignored tracked
   path such as `artifacts/phase-ak/AK11-m5-crosshost-proof/`. Include a
   README, machine-readable manifest, normalized command/config transcript,
   sender/receiver result identifiers, durable-marker observations, and nudge
   observations. The raw ignored smoke report may be referenced by run ID but
   is not the only retained proof artifact.

## Type and boundary inventory

| Item | AK.11 role |
| --- | --- |
| `PeerMessageArray` | Existing AK.8 request envelope exercised physically; AK.11 does not change its schema. |
| `send_peer_http_batch` | Existing AK.9 sender exercised as immediate singleton and recovered array; AK.11 does not add a transport. |
| `MessageStore::confirm_peer_delivery_batch` | Existing atomic cursor operation observed through durable marker evidence; AK.11 does not change storage semantics. |
| Receiver post-write nudge | Existing best-effort post-commit effect. Its failure remains non-fatal to receive success. |
| `scripts/smoke/run_feature_smoke.py` | May gain deterministic evidence-bundle export only if needed; the physical M5 run remains required. |

## Deliverables

1. Add or refine a deterministic proof harness only where needed to emit the
   sanitized tracked evidence manifest. It must use the public send/read/ack
   interface and the existing peer route; it may not bypass daemon admission or
   inspect secrets.
2. Execute the physical M4/M5 run described above after AK.6 is merged and
   commit its sanitized evidence bundle with the exact accepted baseline SHA.
3. Update `docs/peer-pair-smoke.md` with the reproducible disabled-cache-first
   procedure and artifact location, without claiming the proof passes until the
   real manifest is committed.
4. Close `AK5-CROSSHOST-PROOF-001` only when reviewers can reproduce or audit
   the retained M5 evidence. If M5 access or a controlled failure cannot be
   obtained, leave the sprint and finding open rather than manufacturing proof.

## Explicit prohibitions

- No start before AK.6 merges to `integrate/phase-ak`, and no reuse of a run
  against an earlier SHA as final evidence.
- No localhost-only, curl-only, mocked, or transient ignored-log closure.
- No treating a receiver nudge error as receive failure, and no sender-side
  nudge for a cross-host delivery attempt.
- No implementation redesign: this sprint proves the accepted AK.8/AK.9
  contract; material behavior drift returns to a separately planned fix.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  pass on the exact accepted baseline before the physical run.
- The tracked M5 manifest proves all disabled-cache-first and enabled-recovery
  assertions, both directions, and the required durable-marker/nudge
  observations.
- Review verifies no secret material or ignored-only report is required to
  assess the proof, then links the artifact when closing the triage finding.

## Dependencies

AK.11 starts only after the AK.6 PR merges to `integrate/phase-ak`. It is
separate from AK.6 so that physical M5 availability, evidence capture, and any
proof-only harness work cannot delay or dilute AK.6's implementation review.
If a proof exposes real behavior drift, stop evidence closure, file the new
defect, and plan a corrective sprint before rerunning AK.11.
