---
phase: AY
kind: release-readiness checklist (not a sprint; no branch, no PR gate)
title: Live macOS and Windows Herdr socket proof
owner: release readiness (Rand schedules; fenix runs rand-m5; FastPC4 operator per P-D)
status: checklist, runs after Phase AY has landed on develop
supersedes: sprint-AY.10-herdr-live-proof.md (r22), docs/plans/phase-ax/sprint-AX.7-herdr-dogfood-evidence.md (superseded 2026-09-05)
---

# Release readiness: live macOS and Windows Herdr socket proof

Ruling (Rand, 2026-09-05): no sprint carries live evidence. This checklist
is the phase's live proof, executed under the repository's release-readiness
process on the develop or release build that contains Phase AY, after the
integrate/phase-ay to develop PR has merged. It is never a sprint deliverable,
an acceptance criterion, a precondition, or a merge gate for any PR. Its
result gates one thing: the release notes' claim of Windows Herdr parity over
the socket transport.

## Prerequisites (owned outside the phase)

- P-C: the FastPC4 Windows `atm-dev` team exists with Herdr installed via
  the official installer; the parked reporter agent has delivered one
  round-trip report to rand-m4 or rand-m5.
- P-D: the FastPC4 operator agent is named (ATM identity, agent kind).
- The build under test is a signed release-readiness build installed with
  `daemon-switch` on both hosts, staged outside `~/Documents` on macOS.

## Procedure

1. Install the same build SHA on rand-m5 and FastPC4. Record atm build SHA,
   Herdr version, host, operator, start/end timestamps. Confirm `atm doctor`
   healthy and the Herdr endpoint record reports `transport: socket` and the
   expected resolved endpoint.
2. Operators capture the matrix below byte-for-byte. Agents may format the
   index and validate files; they never author, synthesize, or retype
   observations (repository rule: evidence artifacts are never authored).
3. Record results in the release-readiness report for that build, one row per
   case (row contract below), with raw artifacts stored where that process
   keeps them. Nothing is committed under `docs/plans/phase-ay/`.
4. Re-run `atm doctor --json` after the negative, recovery and update cases;
   the final capture shows the breaker closed, the new Herdr version after
   update, every configured endpoint healthy, and no aggregate `herdr.state`
   or `herdr.remedy`.

## Matrix (both platforms unless noted)

- prompt, wait, get, list, notify round trips (request and response JSON);
- endpoint stopped or absent while the daemon stays ready and tmux/hermes
  nudges are unaffected;
- breaker open and recovery; agent not found; agent blocked;
- slow call at the 5 s cap with no orphaned child;
- Herdr starts after atm has been running;
- `herdr update` while nudges continue without a daemon restart;
- socket-boundary structured logs; latency samples;
- one cross-host nudge with timestamps captured at both ends;
- Windows only: the full named-pipe endpoint, `tasklist` before/after the
  timeout case, operator confirmation that no console window flashes; these
  also fill the audit cells AY.7 marked `release readiness` in
  `docs/atm-herdr/windows-process-audit.md`.

## Row contract

```text
case_id | platform | host | build_sha | herdr_version | started_at |
finished_at | request_artifact | response_artifact | expected | observed |
pass/fail | operator
```

Cross-host latency is calculated from the two captured source timestamps.
Exclude secrets, config contents, unrelated message bodies, and terminal
output outside the named case.

## Outcome

- Every row PASS on both hosts: release notes may claim Windows Herdr parity
  over the socket transport.
- Any row FAIL: the release-readiness report records it; a code fix is a new
  sprint or a fix PR on develop through the normal gates. Nothing here is
  patched in place, and Phase AY's disposition (taken on AY.9's automated
  gates) is not reopened by this checklist.
