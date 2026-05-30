# ADR-010 — Claude JSONL Compatibility Envelope

| Field | Value |
|---|---|
| ID | ADR-010 |
| Status | **Accepted** |
| Date | 2026-05-09 |
| Deciders | Rand Lee |
| Relates to | REQ-CORE-COMPAT-001, REQ-CORE-MAILBOX-001, REQ-P-RELIABILITY-001, ADR-009 |
| Supersedes | — |

---

## Context

SQLite-backed durable storage removes the old practical mailbox-size ceiling,
but Claude Code JSONL inboxes remain a tighter operational interface than the
durable store:

- JSONL is a compatibility ingress/egress surface, not ATM's durable source of
  truth
- Claude-facing tooling is more sensitive to message size than SQLite-backed
  storage
- large ATM-authored message bodies can flood operator context or exceed
  practical Claude tooling envelopes even when they are valid durable ATM mail

Phase S.8 therefore needs an explicit compatibility-envelope rule for ATM
authored JSONL export.

## Decision Drivers

- ATM must keep durable full-message fidelity in SQLite
- routine Claude inbox projection must stay small enough for operational use
- oversized ATM-authored messages must remain retrievable without copying the
  full body into Claude JSONL
- compatibility projection updates must not create self-induced watcher churn
- malformed JSONL must remain an ingress issue, not an ATM-authored durable
  store behavior

## Decision

ATM keeps the full ATM-authored message body in SQLite and exports a bounded
compatibility projection to Claude JSONL.

### Export Limit

The default ATM-authored JSONL body export limit is `128 KiB`.

ATM must expose a configuration setting:

- `[atm].claude_jsonl_body_export_max_bytes`

Allowed values:

- `0`: never export ATM-authored full bodies to Claude JSONL
- positive integer: export the full ATM-authored body only when it is less than
  or equal to that limit

### Oversized ATM-Authored Message Projection

When an ATM-authored message body exceeds the configured JSONL export limit:

- the full body remains in SQLite
- the Claude JSONL `text` field is replaced with exactly:
  - `atm read --message-id <id>`
- the message summary remains populated
- ATM must not use this projection rule to smuggle new ATM-owned machine state
  into shared Claude JSON

This projection rule applies only to ATM-authored exports. ATM must not rewrite
Claude-native inbound messages into stub form.

### Watcher / Reconcile Rule

JSONL remains a compatibility projection. Watcher/reconcile logic must treat a
re-observed ATM-authored projection for the same logical message as idempotent
state, not as new logical mail.

Projection updates that only restate the same ATM-owned logical message,
including the same retrieval stub for the same `message_id`, must not trigger
new-mail churn loops.

### Integrity Rule

ATM-authored JSONL exports must always remain valid JSONL records. Malformed
JSONL is tolerated only as an external ingress compatibility problem. Malformed
durable SQLite message rows remain corruption or store-failure conditions.

### Validation

The configured `[atm].claude_jsonl_body_export_max_bytes` value is capped at
`1,048,576` bytes (`1 MiB`).

Rationale:

- it keeps the compatibility-envelope override bounded even when operators
  raise it above the default `128 KiB`
- it preserves JSONL as a compatibility surface rather than a second durable
  large-body transport
- it prevents configuration drift from turning Claude-facing export into an
  effectively unbounded mirror of durable ATM message bodies

## Consequences

### Positive

- large ATM-authored messages stay durable without flooding Claude inbox
  context
- operators have one stable retrieval command for oversized ATM-authored
  messages
- queue inspection and Claude-facing inbox projection stay bounded
- watcher/reconcile behavior can be tested against one explicit no-churn rule

### Negative

- operators may need to use ATM to fetch full bodies that are no longer
  mirrored into Claude JSONL
- export behavior now depends on one explicit configuration setting
- send/export code must distinguish ATM-authored messages from Claude-native
  compatibility messages

## Follow-Up Work

- document the compatibility-envelope rule in product, architecture, and
  crate-local docs
- implement the JSONL export limit config and retrieval-stub projection
- keep watcher/reconcile logic projection-aware so ATM-authored exports do not
  re-enter as new logical messages
