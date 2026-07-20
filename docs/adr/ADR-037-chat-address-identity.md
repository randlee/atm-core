# ADR-037 — Chat Address Identity

| Field | Value |
| --- | --- |
| ID | ADR-037 |
| Status | Accepted for Phase AI planning |
| Scope | Repository-wide |
| Relates to | ADR-033, ADR-035, Phase AH, Phase AI |

## Context

One logical ATM agent can have several independent live contexts. For example,
Hermes can run `hendrix:12345` and `hendrix:98765` at the same time. A message
to either context must nudge, read, and reply through that same context; a
shared `hendrix` mailbox is insufficient.

The earlier Phase AH draft calls this value `session_id` and treats it as a
message field. That name and ownership are misleading: it identifies an agent
context, not a daemon session or a message-thread record.

## Decision

ATM uses an optional **chat-id** as part of an agent address:

```
<agent>[:<chat-id>]@<team>[.<host>]
```

Examples:

```
hendrix:12345@hermes
hendrix:98765@hermes
arch-ctm@atm-dev
```

`agent`, `team`, and `host` retain their existing meanings. `chat-id` is an
opaque, durable identifier for one agent context. Absence denotes the base
agent identity; it is not equivalent to any present chat-id.

`chat-id` is not a message thread id, daemon session id, or a substitute for a
message's immutable ULID. If ATM later needs first-class conversation
threading, that is a separate message relation with a separate contract.

The canonical `AgentAddress`/participant type owns parsing, rendering,
validation, equality, and storage projection. Agent and team segments remain
restricted to letters, digits, `_`, and `-`; `.` and `:` remain reserved
address delimiters. Chat IDs use the same safe segment alphabet so an address
has one unambiguous grammar. AI.1's baseline already contains the completed
Phase AG central validator, `atm_storage::validate_path_segment`; Phase AI
extends that one validator and does not reimplement validation in the CLI,
graft, or HTTP adapter. A future Phase AH Python binding consumes this same
validator and address type rather than adding its own policy.

Every persisted message has nullable `source_chat_id` and
`destination_chat_id` columns beside its existing source/destination agent,
team, and host columns. The columns are optional, independently preserved, and
are not encoded into an agent-name column. All inbox visibility, owner-only
mutation, acknowledgement targeting, nudge display, and reply construction use
that stored address unchanged. At every agent-facing boundary, rendering
concatenates a present chat ID as `agent:chat-id`: a read shows it in `from`, a
write accepts or displays it in `to`, and a nudge from
`hendrix:12345@hermes` displays that address. An acknowledgement or reply
targets `hendrix:12345@hermes`, never a collapsed `hendrix@hermes` identity.

The REST API represents source and destination as structured addresses, not a
synthetic session header. The local CLI and graft populate the same
caller-address contract; a future Phase AH Python binding uses that contract.
The canonical write handler and post-write router do not branch on chat-id.
HTTPS preserves the same request schema.

Message search is explicitly broader than the caller's chat identity:
`atm read --agent hendrix` searches messages involving `hendrix` across all
chat IDs, while `atm read --agent hendrix --chat 12345` narrows that search to
the `hendrix:12345` address component. The equivalent REST collection filter
uses separate `agent` and `chat_id` query fields. No `--session` query or
session-scoped mailbox contract is introduced.

## Consequences

- A Python/Hermes binding can bind one live context to one chat-id without an
  ambient `HERMES_SESSION_KEY` changing mailbox semantics.
- The Phase AH plan must replace its `session_id` protocol and query design
  with this address contract before implementation.
- Chat-aware storage and address tests precede REST/UDS migration because the
  API cannot safely expose an identity it cannot persist and query exactly.
- No separate chat delivery, reply, ack, nudge, or remote routing path is
  permitted.
