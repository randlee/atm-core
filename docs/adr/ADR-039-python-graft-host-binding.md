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

AI.18 exposes the full supported `atm-graft` host contract with PyO3/Maturin:
client operations, session activation and close lifecycle, session snapshots,
and the existing `HostNudgeInjector` callback. The Python surface receives and
returns canonical projections containing structured addresses and immutable
message IDs. A registered Python nudge callback is only a typed translation of
the existing graft callback; it does not create a Python transport, callback
retry queue, or second delivery path. The binding calls the existing sealed
`DaemonApiClient` boundary through graft; it never opens a daemon socket or
accesses storage directly.

Any session/chat-based host may map a validated `ATM_CHAT_ID` to ADR-037
`ChatId` before caller address construction. Hermes is the first host adapter.
Its current profile/session binding and non-interrupting nudge handoff are
governed by ADR-043: an ATM wake-up is not an ordinary inbound user message.
No `session_id`, custom session header, webhook-specific address grammar, or
alternate send/ack path exists.

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
- The canonical roster distinguishes `hermes` from `python-graft`: the former
  names the Hermes gateway adapter, while the latter is available to any
  Python host built on the `atm-graft` interface. Both values select the
  non-Claude graft delivery path and allow post-send nudges without a tmux
  pane.
- AI.19 consumes the complete AI.18 Python host surface and remains a
  Python-only adapter; it does not add Rust wrapper code.
