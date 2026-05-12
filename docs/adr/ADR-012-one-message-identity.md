# ADR-012 — One Message Identity

| Field | Value |
| --- | --- |
| ID | ADR-012 |
| Status | proposed |
| Date | 2026-05-12 |
| Deciders | arch-ctm, team-lead |
| Relates-to | ADR-005, ADR-010 |
| Supersedes | — |

## Context

ATM currently has multiple identity representations in flight:

- `AtmMessageId` — the ATM logical message identity, represented in code as a
  ULID
- Claude Code `message_id` — the UUID wire encoding required at the Claude
  boundary
- `legacy_message_id` — the current SQLite compatibility column retained from
  the earlier split-identity design

`metadata.atm.messageId` must not be conflated with `LegacyMessageId`. Under
the approved Phase U direction, ATM keeps one logical identity, uses UUID only
as Claude boundary wire encoding, and does not retain a duplicated ATM-owned
durable identity field.

This split creates duplicated storage, ambiguous query paths (which id do you use to look up a message?), and implicit truth dependencies on the Claude JSON envelope.

The Phase U cleanup decisions establish:
- ATM has one logical message identity: `AtmMessageId` (ULID)
- Claude Code `message_id` (UUID) is treated as boundary wire encoding only;
  it is cast to ULID at ingestion and not retained as a second ATM-owned
  durable identity
- No second ATM-owned durable identity field may exist for the same logical message

## Decision

ATM keeps one logical message identity: `AtmMessageId`, represented in code as
ULID.

The Claude Code `message_id` field is the required UUID wire encoding at the
Claude boundary only. ATM may cast that UUID payload to `AtmMessageId` at
ingress and cast `AtmMessageId` back to UUID at the Claude export boundary, but
the UUID form is not a second ATM-owned durable identity.

`LegacyMessageId` is removed from the target design as an independently owned
identity concept. The current SQLite `legacy_message_id` compatibility column
is transitional removal work under Phase U and must not survive as a second
durable ATM identity after U.2 lands.

`metadata.atm.messageId` is also removed from the target design. ATM must not
persist or query a second ATM-owned message-id field inside the Claude
compatibility envelope.

Approved rule set:
- one logical ATM message identity: `AtmMessageId`
- UUID <-> ULID reinterpretation occurs only at the Claude compatibility
  boundary
- no dual-id query paths
- no dual-id durable storage
- no ATM-owned fallback or repair logic that reintroduces a second message-id
  field

## Consequences

Required implementation consequences:
- dual-id code paths are removed from send/read/ack/threading and SQLite query
  logic
- `legacy_*` identity persistence is removed or reduced to bounded migration
  handling only
- Claude compatibility ingest/export remains supported through the approved
  UUID/ULID boundary cast
- future ATM features must use `AtmMessageId` as the only ATM-owned message
  identity

Migration consequence:
- any host carrying pre-U.2 `legacy_message_id` rows must migrate or rebuild
  those rows as part of the U.2 cleanup line rather than preserving the split
  identity model indefinitely
