---
title: Phase AI cross-host evidence branch disposition
status: proposed
source_branch: evidence/phase-ai-crosshost-smoke
authoritative_plan: docs/plans/phase-ai/sprint-ai-21-pre-crosshost-evidence-harness.md
---

# Evidence branch disposition

`evidence/phase-ai-crosshost-smoke` is an investigation branch, not a merge
source. Its mixed implementation and artifacts are retained only as evidence
and candidate inputs. No code from it may be cherry-picked merely because a
smoke experiment appeared to work.

## Keep through named sprint adoption

| Evidence asset | Adoption sprint | Required condition |
| --- | --- | --- |
| `scripts/smoke/run_inbound_peer_smoke.py`, `combine_inbound_peer_smoke.py`, `analyze_logs.py`, example config, and XHTML templates | AI.21-pre | Adopt as the one supported Python/sc-compose runner; add deterministic JSON/XHTML/combiner tests. |
| Explicit plaintext peer diagnostic concept | AI.21-pre | Reimplement only as `atm-daemon --peer-wire-security plaintext-test` under `REQ-CORE-TRANSPORT-002B1`; same canonical HTTP path, no environment switch, no automatic fallback. |
| Hostname/current-IP diagnostics | AI.25 | Reimplement by evolving DNS-backed `TrustedPeer`; never persist IP aliases or use reverse DNS. |
| Shared canonical write, host-qualified self-IP, and ACK findings | AI.22–AI.24 | Accept only the behavior covered by their approved plans and gates; do not copy experimental helper paths. |
| Deadline/outcome/recovery observations | AI.26–AI.28 | Reimplement only through the single write route, AI.27 telemetry, and AI.28 single-flight coordinator. |

## Retain as diagnostic artifacts only

`artifacts/peer-smoke/**`, raw logs, curl captures, handoff IDs, and the
consolidated root-cause reports explain prior observations. They are not test
fixtures, production configuration, or proof that a later branch works.

## Discard as an implementation source

- all ad-hoc daemon lifecycle/symlink handling;
- environment-driven peer-security selection;
- any test-only source-host header treated as authentication;
- direct transport-to-storage/nudge logic, alternate request envelopes, or
  peer-specific ACK behavior;
- durable replay/outbox/receipt/per-message retry code;
- unrelated Phase AI/Hermes changes that already have an authoritative merged
  sprint branch.

## QA rule

QA reviews implementation only against the named AI.21-pre–AI.30 sprint plan,
requirements, and ADRs. Evidence artifacts may explain why a test exists, but
cannot expand scope or override a requirement. Every retained evidence-derived
behavior must have a named plan deliverable, acceptance criterion, and test.
