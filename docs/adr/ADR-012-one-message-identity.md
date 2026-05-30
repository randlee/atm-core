# ADR-012 — One Message Identity

| Field | Value |
| --- | --- |
| ID | ADR-012 |
| Status | accepted |
| Date | 2026-05-12 |
| Deciders | arch-ctm, team-lead |
| Relates-to | ADR-005, ADR-010 |
| Supersedes | — |

## Context

ATM had accumulated multiple message-identity representations:

- `AtmMessageId` in ATM code
- Claude Code `message_id` at the shared inbox boundary
- `metadata.atm.messageId` inside the compatibility envelope
- `legacy_message_id` / `LegacyMessageId` compatibility naming in the earlier
  SQLite line

That shape created duplicated storage, ambiguous query paths, and confusing
ownership. Phase U resolves that by keeping one logical ATM identity and
treating Claude's UUID form as boundary encoding only.

## Decision

ATM keeps one logical message identity: `AtmMessageId`.

Rules:
- `AtmMessageId` is the only ATM-owned message identity in code.
- Claude Code `message_id` is the UUID wire encoding of that same identity at
  the shared inbox boundary.
- ATM may cast UUID wire values into `AtmMessageId` on ingest and cast
  `AtmMessageId` back into UUID wire form on export.
- ATM must not persist or query a second ATM-owned message-id field for the
  same logical message.
- `metadata.atm.messageId` is removed from the design and implementation.
- `LegacyMessageId` and `legacy_*` naming are removed or narrowed away from the
  active identity model.
- CLI and service addressing may accept either ULID text or UUID wire text, but
  both resolve to the same `AtmMessageId`.

SQLite consequence:
- if SQLite stores a durable `message_id` field, that field stores the same
  logical identity in compatibility UUID wire form only; it is not a second
  ATM-owned identity.

## Consequences

Required implementation consequences:
- dual-id code paths are removed from send/read/ack/threading and SQLite query
  logic
- `metadata.atm.messageId` is deleted
- `legacy_*` identity naming is removed from the active implementation path
- `crates/atm-core/src/workflow.rs` may continue to accept `legacy:` workflow
  sidecar keys as a read-compatibility shim only; all new writes use `atm:`,
  and the shim can be removed once older workflow-state files no longer need
  to be read in place
- Claude compatibility ingest/export remains supported through the approved
  UUID-wire boundary cast
- future ATM features must use `AtmMessageId` as the only ATM-owned message
  identity
