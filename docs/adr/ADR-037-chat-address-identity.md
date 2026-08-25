# ADR-037 — Chat Address Identity

| Field | Value |
| --- | --- |
| ID | ADR-037 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-033, ADR-035, Phase AI |

*Terminology note (Phase AQ): 'nudge' below means the steer (immediate) kind; see the nudge taxonomy in docs/requirements.md.*

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
graft, or HTTP adapter. The AI.18 Python binding consumes this same validator
and address type rather than adding its own policy.

The shared identity parser is `AgentIdentity::from_str` for
`agent[:chat-id]`; `agent:XXX` therefore always parses as agent `agent` with
`chat_id=Some(XXX)`. `AgentAddress::from_str` parses the full address by
splitting once at `@` and delegating its left component to `AgentIdentity`.
CLI caller overrides, graft, HTTP, storage projection, and nudge rendering use
these types rather than independently interpreting `:`.

CLI composition follows that grammar: base agent plus `--team <team>` is the
logical `agent@team`; adding `--chat-id XXX` is the logical
`agent:XXX@team`. The adapter normalizes those parts to the same
`AgentAddress` as the textual address before dispatch. Caller chat-id
precedence is: `--as` (including explicit absence), then `--chat-id`, then
`ATM_CHAT_ID`, then an embedded `agent:chat-id` in `ATM_IDENTITY`, then no
chat-id. `ATM_CHAT_ID` never supplies an agent; it requires `ATM_IDENTITY`.

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
synthetic session header. The local CLI, graft, and AI.18 Python binding
populate the same caller-address contract.
The canonical write handler and post-write router do not branch on chat-id.
HTTPS preserves the same request schema.

The CLI accepts a chat-qualified recipient only in the canonical spelling
`<agent>:<chat-id>@<team>[.<host>]`. For caller context it accepts either
`atm send <to> --chat-id <chat-id> <message>` or `atm send <to> --as
<agent>:<chat-id> <message>`: with `ATM_IDENTITY=omega-prime`, both produce
caller `omega-prime:<chat-id>`. `--chat-id` and `--as` are mutually exclusive;
the caller team continues to come from `--team` or `ATM_TEAM`. `--from`
remains a read/list sender filter and is never a write caller override. No
variant creates a second parsing, wire, storage, or routing path.

The same caller-context shorthand applies to owner-only reads: with
`ATM_IDENTITY=omega-prime`, `atm read --chat-id <chat-id>` and `atm read --as
omega-prime:<chat-id>` select the same context mailbox. `ATM_CHAT_ID` and an
embedded chat-id in `ATM_IDENTITY` provide the same caller context at lower
precedence. This changes only the
resolved `AgentAddress`; it does not create a chat-specific read path.

Message search is explicitly broader than the caller's chat identity:
`atm read --agent hendrix` searches messages involving `hendrix` across all
chat IDs, while `atm read --agent hendrix --chat 12345` narrows that search to
the `hendrix:12345` address component. The equivalent REST collection filter
uses separate `agent` and `chat_id` query fields. No `--session` query or
session-scoped mailbox contract is introduced.

## Consequences

- The AI.17–AI.21 Python/Hermes integration can bind one live context to one
  chat-id through ambient `ATM_CHAT_ID` without changing mailbox semantics.
- AI.17–AI.21 must consume this address contract and must not introduce a
  `session_id` protocol or query design.
- Chat-aware storage and address tests precede REST/UDS migration because the
  API cannot safely expose an identity it cannot persist and query exactly.
- No separate chat delivery, reply, ack, nudge, or remote routing path is
  permitted.
- `--chat-id` and its equivalent `--as` caller spelling must yield identical
  request addresses before the shared write path.
