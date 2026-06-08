# SQL Server Readiness Proof

## Scope

This is the final `AC.7` proof against the actual post-`AC.6` codebase at
`feature/pAC-s7-sqlserver-readiness-proof`, based on `AC.6` tip `4e5ffcc2`.

It proves two things:

- the shared `atm-storage` contract is backend-neutral enough for a future SQL
  Server backend
- the remaining work for SQL Server is backend implementation work, not another
  storage-architecture reset

This is not a production SQL Server backend. The compile-only proof crate
`crates/atm-storage-sqlserver-proof` exists to demonstrate trait compatibility
and crate-graph cleanliness only.

## Final Shared Contract Surface

The audited `atm-storage` public surface is small enough to review directly:

- `3` public storage traits:
  - `MessageStore`
  - `RosterStore`
  - `StorageNotifier`
- `12` canonical storage/domain contract records and enums:
  - `MessageKey`
  - `TaskState`
  - `AckTransition`
  - `Message`
  - `MessageQuery`
  - `RosterMemberKind`
  - `RosterHarness`
  - `AgentType`
  - `RosterMember`
  - `RosterSnapshot`
  - `MessageReceivedEvent`
  - `RosterChangedEvent`
- shared storage/schema support types remain in `atm-storage`, but the storage
  contract itself stays CRUD-shaped and semantic instead of request/response
  wrapper-shaped

Important shape checks from the post-`AC.6` tree:

- no request/response-per-operation storage wrappers remain in
  `crates/atm-storage`, `crates/atm-storage-claude`, or
  `crates/atm-storage-rusqlite`
- no Claude mailbox file concepts are required by the shared contract
- no SQLite observability or assembly types are required by the shared contract
- speculative task-store surfaces are not part of the approved shared contract

## Backend-Neutrality Proof

The current backend graph now supports peer backends:

- `atm-storage-claude -> atm-storage`
- `atm-storage-rusqlite -> atm-storage`
- `atm-storage-sqlserver-proof -> atm-storage`

And it forbids the old architectural drift:

- no backend crate depends on `atm-core`
- `atm-daemon-client` depends on `atm-storage`, not on either concrete backend
- the shared contract does not encode Claude JSON array mechanics
- the shared contract does not encode SQLite-only lifecycle or observability
  concepts

The proof crate lands compileable backend stubs that implement the same
contract as the Claude and SQLite backends:

- `SqlServerMessageStore: MessageStore`
- `SqlServerRosterStore: RosterStore`

That crate compiles without an `atm-core` edge, which is the direct evidence
that a future SQL Server backend can be introduced as a peer backend rather
than as another architecture exception.

## What A Future `atm-storage-sqlserver` Must Implement

A real SQL Server backend must implement exactly the existing shared storage
contract:

- `MessageStore`
  - `save_message`
  - `load_message`
  - `list_messages`
  - `delete_message`
- `RosterStore`
  - `load_roster`
  - `save_roster`
  - `list_teams`

Canonical shared structs and identifiers it must accept directly:

- `Message`
- `MessageKey`
- `MessageQuery`
- `RosterMember`
- `RosterSnapshot`
- `TeamName`
- `AgentName`
- `MessageEnvelope`
- `PendingAck`
- `AtmMessageId`

Optional follow-up work remains implementation-specific:

- SQL Server connection/session management
- DDL and migration layout
- transactional batching or isolation tuning
- index strategy for `list_messages` and roster lookups
- any backend-specific observability that stays below the trait line

Those are backend details, not shared-contract defects.

## Concerns Explicitly Outside The Shared Contract

These existing concerns do not block SQL Server because they are already kept
outside the contract:

- Claude inbox file layout, salvage, and rewrite mechanics
- Claude roster projection details
- SQLite shared-db plumbing
- SQLite observability internals
- transport-layer `RpcEnvelope` ownership in `atm-daemon-client`
- speculative task-store lines deleted during `AC.6`

Because those concerns are below the trait line, SQL Server does not need to
inherit or emulate them to participate in the storage model.

## Remaining Work Checklist For A Real SQL Server Backend

1. Rename the proof crate line into a real `atm-storage-sqlserver` crate.
2. Replace compile-only error returns with real SQL Server-backed persistence.
3. Add integration tests that round-trip canonical `Message` and
   `RosterSnapshot` records.
4. Add backend-internal migrations and connection management.
5. Add boundary TOMLs for the concrete SQL Server backend crate once it lands.
6. Decide whether any optional capability trait is actually needed for SQL
   Server, rather than adding one by default.

None of those steps require redefining the `atm-storage` contract or moving
business logic back above the storage seam.

## Conclusion

Phase `AC` closes with the shared storage contract explicitly ready for a
future SQL Server backend.

The proof is no longer hypothetical:

- the contract is small and directly auditable
- the backend graph no longer forces `atm-core` into backend crates
- a compile-only SQL Server proof crate implements the existing traits today
- the remaining work is normal backend implementation scope

That means future SQL Server work can start from the current contract rather
than reopening the storage-architecture reset.
