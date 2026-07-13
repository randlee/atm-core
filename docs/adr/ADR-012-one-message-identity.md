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
- `metadata.atm.messageId` inside the compatibility envelope
- `legacy_message_id` / `LegacyMessageId` compatibility naming in the earlier
  pre-production SQLite line

That shape created duplicated storage, ambiguous query paths, and confusing
ownership. The accepted Phase AD line resolves that by keeping one logical ATM
identity and retiring UUID compatibility along with the Claude JSON backend.

## Decision

ATM keeps one logical message identity: `AtmMessageId`.

Rules:
- `AtmMessageId` is the only ATM-owned message identity in code.
- `AtmMessageId` is encoded as ULID text on retained ATM boundaries.
- ATM must not persist or query a second ATM-owned message-id field for the
  same logical message.
- `metadata.atm.messageId` is removed from the design and implementation.
- `LegacyMessageId` and `legacy_*` naming are removed from the active identity
  model. Remaining references may survive only as historical planning or
  removal-ledger context, not as runtime compatibility.
- CLI and service addressing accept ULID text only.

SQLite consequence:
- if SQLite stores a durable `message_id` field, that field stores the same
  logical identity in ULID text only; it is not a second ATM-owned identity.

## Consequences

Required implementation consequences:
- dual-id code paths are removed from send/read/ack/threading and SQLite query
  logic
- `metadata.atm.messageId` is deleted
- `legacy_*` identity naming is removed from the active implementation path
  and from normal 1.2 SQLite bootstrap/migration support
- `crates/atm-core/src/workflow.rs` may continue to accept `legacy:` workflow
  legacy workflow-sidecar keys as a historical read-compatibility shim only;
  that shim is retired because SQLite is now the sole mailbox-state authority
- UUID compatibility code is removed from retained CLI, daemon, storage, and
  schema/tooling paths
- future ATM features must use `AtmMessageId` as the only ATM-owned message
  identity

Implementation status:
- `AA.11` closes the SQLite-side `legacy_*` consequence on
  `feature/pAA-s11-delete-sqlite-legacy-compat`.
- No `legacy_message_id` code paths remain under `crates/atm-rusqlite/`; any
  surviving `legacy_*` references are historical planning or removal-ledger
  context only.
- `AD.13` closes the retained UUID-compatibility consequence on
  `feature/pAD-s13-ulid-message-identity-reset`.
- The accepted ATM line no longer depends on the `uuid` crate, no retained
  `AtmMessageId` path accepts or emits UUID-form message ids, and retained
  schemas/tooling document ULID-only ATM message identity.
