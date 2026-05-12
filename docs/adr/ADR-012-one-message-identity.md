# ADR-012 — One Message Identity

| Field | Value |
| --- | --- |
| ID | ADR-012 |
| Status | Stub — written by sprint U.2 |
| Date | TBD (U.2 sprint) |
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

_This ADR stub reserves the ID. The full decision record, rationale, consequences, and implementation notes are written during sprint U.2._

Required content for the full ADR:
- formal definition of the one-message-identity rule
- the approved UUID→ULID boundary cast at ingestion
- the prohibited patterns (dual id storage, `legacy_*` persistence, `metadata.atm.messageId` write)
- migration notes for any host with existing `legacy_message_id` rows

## Consequences

_Written by sprint U.2._
