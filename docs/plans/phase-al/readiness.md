---
title: Phase AL readiness
status: blocked
---

# Phase AL readiness

## Candidate status

- **SOURCE-READY (active Tokio/Axum runtime):** yes. The candidate's daemon
  executable composes `atm-http-runtime`; retained synchronous daemon source
  is reference-only and is not an active listener or publisher.
- **PHYSICAL-PROOF-BLOCKED (Phase AL completion):** yes. The authoritative
  Phase AL completion gate remains blocked until AL.9's final physical-proof
  matrix runs against one frozen candidate and satisfies every required row.

This is deliberately not a `complete` declaration. A source-clean candidate,
unit tests, or historical host evidence cannot substitute for the required
final physical proof.

## Close-out record

| Item | State | Evidence |
| --- | --- | --- |
| AL-CLOSURE-002 | corrected | `d2e1cc04`: AL.1/AL.3 completion metadata reconciled; unimplemented AL.7 TLS scope explicitly abandoned and quarantined |
| AL-CLOSURE-003 | corrected | `4ffbacbc`: `docs/project-plan.md` records AL.10-AL.15 historical and current outcomes consistently |
| RBQA-F101-AL | corrected | `09f4b0cd`: `atm-http-runtime` owns write-connector selection and its deadline; CLI/graft delegate |
| RBQA-F102-AL | corrected | `258ee362`: the architecture gate now enforces `atm-http-runtime` as the sole write-transport owner |
| RBQA-F103-AL | corrected/waived | `9e303af7`: active Tokio bootstrap and CLI use the `atm-core` contract; the frozen legacy daemon occurrence is waived for Phase AM deletion |
| AL-CLOSURE-004 | pending physical proof | intentionally out of this documentation/source cleanup; final AL.9 physical-proof run is required |

**2026-08-28 (out-of-band, PR #1082, `fix/aq-phase-review-boundary`):** an
undeclared Cargo dependency edge from `atm-graft` to `atm-http-runtime` was
backfilled into `boundaries/atm-graft/shared-client-consumer.toml`, guarded by
the new architecture test
`al9_atm_graft_pins_full_dependency_set_including_http_runtime`. This is a
boundary-declaration correction only, made outside the Phase AL sprint
sequence during Phase AQ review. **AL-CLOSURE-004 remains open and is
unaffected by this change** — the AL.9 final physical-proof matrix is still
required before Phase AL can be declared complete.

## Required physical proof before completion

At a single SHA/version-selected candidate, retain and index the following:

| Proof | Required result |
| --- | --- |
| macOS local | managed runtime doctor, localhost/local-IP smoke, UDS and TCP benchmark reports |
| M5↔M4 direct peer | bidirectional send/read and requires-ack/reply, plus M5 benchmark and graft proof |
| Windows local | managed runtime doctor and local benchmark/report |
| Runtime ownership | exactly one active listener and endpoint-record publisher for every enabled endpoint |
| Regression | `just lint`, `just test`, and the Phase AL architecture gates pass at the frozen candidate |

The final closeout must link all retained reports from `site/reports/index.html`
and name the exact candidate SHA, version, hosts, and report paths.

## Windows cross-host infrastructure waiver

Windows-originated M4/M5 direct-peer proof is deferred indefinitely: the
available Windows operator has neither the required VPN route/DNS resolution
nor reachable M4/M5 hardware. This is an infrastructure limitation, not an
ATM runtime or Windows-code defect, and no SSH, raw-IP configuration, second
daemon, or alternate runner may be introduced to simulate it. The waiver does
not reduce the required Windows local doctor/benchmark evidence or the M5↔M4
direct-peer proof. Quality management owns the corresponding triage-record
disposition.
