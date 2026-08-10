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
| RBQA-F102-AL | follow-up | the architecture assertion still describes the former F101 duplication and must be rewritten only after the F101 change is available to the reviewer |
| RBQA-F103-AL | disposition required | its requested edit includes frozen legacy `atm-daemon` source. The active Tokio/Axum path may not patch that retained reference source; QA must record a waiver or an active-runtime-only replacement scope. |
| AL-CLOSURE-004 | pending physical proof | intentionally out of this documentation/source cleanup; final AL.9 physical-proof run is required |

## Required physical proof before completion

At a single SHA/version-selected candidate, retain and index the following:

| Proof | Required result |
| --- | --- |
| macOS local | managed runtime doctor, localhost/local-IP smoke, UDS and TCP benchmark reports |
| M5↔M4 direct peer | bidirectional send/read and requires-ack/reply, plus M5 benchmark and graft proof |
| Windows local and direct peer | managed runtime doctor, local benchmark/report, direct public-CLI evidence correlated with M4 |
| Runtime ownership | exactly one active listener and endpoint-record publisher for every enabled endpoint |
| Regression | `just lint`, `just test`, and the Phase AL architecture gates pass at the frozen candidate |

The final closeout must link all retained reports from `site/reports/index.html`
and name the exact candidate SHA, version, hosts, and report paths.
