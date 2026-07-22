# ADR-039 — Python Graft Host Binding

| Field | Value |
| --- | --- |
| ID | ADR-039 |
| Status | Accepted |
| Scope | `atm-graft` Python hosts, including Hermes |
| Relates to | ADR-033, ADR-035, ADR-037, AI.17–AI.21 |

## Context

Hermes hosts agent loops in Python while `atm-graft` is a Rust client and
nudge-receiver library. A Python integration must not recreate daemon access,
message parsing, persistence, or routing merely to bridge that language
boundary.

## Decision

AI.18 exposes the existing typed graft contract with PyO3/Maturin. The Python
surface receives and returns canonical projections containing structured
addresses and immutable message IDs. It calls the existing sealed
`DaemonApiClient` boundary through graft; it never opens a daemon socket or
accesses storage directly.

AI.17 maps a validated `HERMES_SESSION_KEY` to ADR-037 `ChatId` before caller
address construction. AI.19 maps a received canonical source address to a
Hermes-local `atm:` chat and injects the nudge body through Hermes's ordinary
inbound-user-message mechanism. No `session_id`, custom session header,
webhook-specific address grammar, or alternate send/ack path exists.

The atm-core deliverable is an in-repository reference adapter and contract.
It does not edit or validate an external Hermes checkout; Hermes maintainers
adopt that contract through their own repository and review process. AI.20
ships a parameterized launchd template and operator runbook, not named
personal-machine profiles.

## Consequences

- `agent:chat-id@team` remains the sole agent-facing identity across CLI,
  graft, Python, nudge, reply, and acknowledgement.
- The daemon persists before post-write nudge emission; Hermes cannot observe
  a nudge for a message unavailable to a normal read.
- Hermes remains an external host integration. Its process supervision and
  chat namespace are not daemon responsibilities.
