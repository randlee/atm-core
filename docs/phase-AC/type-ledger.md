# Phase AC Type Ledger

## Goal

Provide the exhaustive `AC.0` ledger of the current storage-adjacent type
surface so later sprints can work from an explicit keep / merge / delete map
instead of reconstructing type scope from scattered notes.

This ledger is exhaustive for the `AC.0` searched public surfaces plus the one
decisive private Claude storage seam in `delivery_execution.rs`.

## Ledger Rules

Disposition labels:

- `retain-shared` — survives into `atm-storage` in some refined form
- `merge-into-shared` — collapsed into a canonical shared type
- `replace-trait` — old trait deleted and replaced by a new `atm-storage`
  trait or capability trait
- `delete-wrapper` — request / response / envelope wrapper deleted
- `backend-only` — stays below the backend trait line
- `out-of-scope-transport` — remains outside the storage contract
- `delete-bundle` — backend-shaped assembly bundle removed
- `capability-candidate` — candidate capability / health / replay type that
  must stay out of the core CRUD contract unless explicitly justified

Planning rule:

- `AC.1` and `AC.5` own shared-type convergence decisions
- `AC.2` owns Claude-backend-only type retention
- `AC.3` owns SQLite-backend-only type retention
- `AC.4` owns core-consumer migration off deleted seams
- `AC.6` owns final wrapper and leakage deletion verification

Final action shorthand used throughout the ledger:

- `move-to-atm-storage` — becomes part of the shared `atm-storage` contract
- `merge-and-delete` — merged into a canonical shared type, old concrete type deleted
- `replace-and-delete` — old trait or seam replaced, old type deleted
- `internalize-claude` — move below `atm-storage-claude` as backend-only detail
- `internalize-rusqlite` — move below `atm-storage-rusqlite` as backend-only detail
- `retain-outside-storage` — remains in the repo but stays outside the storage contract
- `capability-review` — only survives if later sprint explicitly keeps it as a small capability type

## Count Summary

Exhaustive entries in this ledger:

- `23` public traits
- `102` public structs
- `4` enums
- `1` decisive private seam trait

Grouped source counts:

| Source | Traits | Structs | Enums |
| --- | ---: | ---: | ---: |
| `boundary/mod.rs` | `9` | `3` | `0` |
| `boundary/mail.rs` | `2` | `35` | `0` |
| `boundary/store.rs` | `9` | `58` | `3` |
| `boundary/runtime.rs` | `2` | `2` | `0` |
| `atm-rusqlite` public support types | `1` | `4` | `1` |

## Expected Reduction Shape

The ledger is intentionally biased toward deletion or scope reduction rather
than relocation.

Expected dominant outcomes:

- request / response wrapper families -> `merge-and-delete`
- backend-shaped bundle helpers -> `replace-and-delete`
- Claude projection and compatibility types -> `internalize-claude`
- SQLite observability and assembly helpers -> `internalize-rusqlite`
- transport / config / outbound seams -> `retain-outside-storage`
- replay / doctor / health seams -> `capability-review`

Planning constraint:

- a type only survives publicly if it is either:
  - part of the small shared `atm-storage` contract, or
  - a deliberate non-storage seam that remains outside the storage boundary
- all other public types are presumed deletion or backend-internalization
  candidates unless a later sprint justifies them explicitly

## `crates/atm-core/src/boundary/mod.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `MessageKey` | struct | `retain-shared` | `move-to-atm-storage` | `atm-storage::MessageKey` in `AC.1` | Must wrap `AtmMessageId` per `ADR-012`. |
| `TaskState` | struct | `retain-shared` | `move-to-atm-storage` | task state newtype / enum in `AC.1` | Keep as semantic state, not backend-shaped wrapper. |
| `AckTransition` | struct | `retain-shared` | `move-to-atm-storage` | shared ack-transition helper in `AC.1` | Shared semantic helper, not backend-specific. |
| `AtmProtocol` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | RPC / protocol boundary, not storage. |
| `ClientTransport` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Transport boundary only. |
| `ServerTransport` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Transport boundary only. |
| `RequestDispatcher` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | RPC dispatch, not storage. |
| `AdvisoryStreamSink` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Advisory stream behavior is not storage CRUD. |
| `NotificationSink` | trait | `out-of-scope-transport` | `retain-outside-storage` | compare against `StorageNotifier` in `AC.4` | Must not be silently reused as the storage notifier without review. |
| `StatusSource` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Runtime status surface, not storage. |
| `WatchEventSource` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Watch surface, not storage. |
| `ReconcileCoordinator` | trait | `out-of-scope-transport` | `retain-outside-storage` | remains outside `atm-storage` | Reconcile workflow, not storage. |

## `crates/atm-core/src/boundary/mail.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `ReplaySource` | struct | `capability-candidate` | `capability-review` | replay capability review in `AC.1` / `AC.3` | Replay is not part of the core CRUD contract by default. |
| `MailStoreMessageRecord` | struct | `merge-into-shared` | `merge-and-delete` | canonical `Message` in `AC.1` / `AC.5` | Main storage message record to collapse. |
| `MailMessageState` | struct | `merge-into-shared` | `merge-and-delete` | shared message-state helper in `AC.1` | Must not remain a separate backend-shaped record. |
| `MessageFingerprint` | struct | `retain-shared` | `move-to-atm-storage` | shared helper / newtype in `AC.1` | Candidate cross-backend helper if still needed. |
| `MailStoreIngestReplayState` | struct | `capability-candidate` | `capability-review` | replay capability in `AC.1` / `AC.3` | Keep out of base CRUD contract unless justified. |
| `MailStoreHealthSnapshot` | struct | `capability-candidate` | `capability-review` | storage health capability in `AC.1` / `AC.3` | Health / doctor surface, not CRUD core. |
| `MailStoreMailboxMetadataRow` | struct | `merge-into-shared` | `merge-and-delete` | `MessageQuery` result helper in `AC.1` / `AC.5` | Metadata must not remain a mail-store-only row type. |
| `MailStoreQueryMailboxMetadataRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper family collapse. |
| `MailStoreQueryMailboxMetadataResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Wrapper family collapse. |
| `MailStoreMailboxMetadataCounts` | struct | `merge-into-shared` | query helper candidate in `AC.1` | Keep only if semantics survive the query redesign. |
| `MailStoreQueryMailboxMetadataCountsRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Wrapper family collapse. |
| `MailStoreQueryMailboxMetadataCountsResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Wrapper family collapse. |
| `MailStoreBootstrapRequest` | struct | `delete-wrapper` | deleted in `AC.4` / `AC.6` | Backend bootstrap must not survive as shared storage DTO. |
| `MailStoreBootstrapResponse` | struct | `delete-wrapper` | deleted in `AC.4` / `AC.6` | Backend bootstrap must not survive as shared storage DTO. |
| `MailStoreTransactionRequest` | struct | `delete-wrapper` | deleted or replaced by capability in `AC.1` / `AC.6` | No RPC-style transaction wrapper in base storage contract. |
| `MailStoreTransactionResponse` | struct | `delete-wrapper` | deleted or replaced by capability in `AC.1` / `AC.6` | No RPC-style transaction wrapper in base storage contract. |
| `MailStoreUpsertMessageRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | `save_message` absorbs this operation. |
| `MailStoreUpsertMessageResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | `save_message` absorbs this operation. |
| `MailStoreLoadMessageRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | `load_message` absorbs this operation. |
| `MailStoreLoadMessageResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | `load_message` absorbs this operation. |
| `MailStoreLoadStoredMessageRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Stored-message distinction must be re-justified or removed. |
| `MailStoreLoadStoredMessageResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Stored-message distinction must be re-justified or removed. |
| `UpsertMailMessageStateRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | State mutation must be represented semantically. |
| `UpsertMailMessageStateResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | State mutation must be represented semantically. |
| `LoadMailMessageStateRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | State lookup must be represented semantically. |
| `LoadMailMessageStateResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | State lookup must be represented semantically. |
| `MailStoreRecordIngestReplayStateRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Replay capability must not use RPC-style wrappers. |
| `MailStoreRecordIngestReplayStateResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Replay capability must not use RPC-style wrappers. |
| `MailStoreLoadIngestReplayStateRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Replay capability must not use RPC-style wrappers. |
| `MailStoreLoadIngestReplayStateResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Replay capability must not use RPC-style wrappers. |
| `MailStoreHealthSnapshotRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Health capability must not use RPC-style wrappers. |
| `MailStoreHealthSnapshotResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Health capability must not use RPC-style wrappers. |
| `MailStoreRequest` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `MailStoreResponse` | struct | `delete-wrapper` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `MailStoreDoctorReport` | struct | `capability-candidate` | storage health / doctor capability in `AC.1` / `AC.3` | Keep only if doctor remains a separate capability shape. |
| `MailStore` | trait | `replace-trait` | `MessageStore` in `AC.1` | Old trait deleted when shared contract lands. |
| `MailStoreDoctor` | trait | `replace-trait` | health / doctor capability in `AC.1` / `AC.3` | Must not survive unchanged into `atm-storage`. |

## `crates/atm-core/src/boundary/store.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `TaskStoreTaskMetadata` | struct | `merge-into-shared` | `merge-and-delete` | canonical `Task` in `AC.1` | Merge into one task model unless narrowly justified. |
| `TaskStoreTaskRecord` | struct | `merge-into-shared` | `merge-and-delete` | canonical `Task` in `AC.1` / `AC.5` | Main task storage record to collapse. |
| `RosterStoreHealthSnapshot` | struct | `capability-candidate` | `capability-review` | storage health capability in `AC.1` / `AC.3` | Not part of CRUD core. |
| `RosterMemberKind` | enum | `retain-shared` | `move-to-atm-storage` | shared enum in `AC.1` | Semantic roster member property. |
| `RosterHarness` | enum | `retain-shared` | `move-to-atm-storage` | shared enum in `AC.1` | Semantic roster harness property. |
| `RosterMemberRecord` | struct | `merge-into-shared` | `merge-and-delete` | canonical `RosterMember` in `AC.1` | Main roster member record to collapse. |
| `ClaudeCodeRosterMember` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude projection type, not shared contract. |
| `ClaudeCodeTeamRoster` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude projection type, not shared contract. |
| `TaskStoreCreateTaskRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreCreateTaskResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreLoadTaskRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreLoadTaskResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreUpdateTaskRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreUpdateTaskResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreAttachMessageLinkRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreAttachMessageLinkResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreDetachMessageLinkRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreDetachMessageLinkResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreRecordAckTransitionRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreRecordAckTransitionResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `TaskStoreQueryTaskMetadataRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Query semantics must collapse into `TaskQuery`. |
| `TaskStoreQueryTaskMetadataResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Query semantics must collapse into `TaskQuery`. |
| `TaskStoreRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `TaskStoreResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `TaskStoreDoctorReport` | struct | `capability-candidate` | `capability-review` | storage health / doctor capability in `AC.1` / `AC.3` | Keep only if doctor remains separate. |
| `RosterStoreReplaceRosterRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `RosterStoreReplaceRosterResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `RosterStoreLoadRosterRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `RosterStoreLoadRosterResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Wrapper collapse. |
| `RosterStoreQueryMembershipRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Query semantics must collapse into shared roster query helpers or disappear. |
| `RosterStoreQueryMembershipResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Query semantics must collapse into shared roster query helpers or disappear. |
| `RosterStoreHealthSnapshotRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Health capability must not use wrapper DTOs. |
| `RosterStoreHealthSnapshotResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Health capability must not use wrapper DTOs. |
| `RosterStoreListTeamsRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | `list_teams` method should not need a request DTO. |
| `RosterStoreListTeamsResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | `list_teams` method should not need a response DTO. |
| `RosterStoreRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `RosterStoreResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` / `AC.6` | Envelope wrapper family must disappear. |
| `RosterStoreDoctorReport` | struct | `capability-candidate` | `capability-review` | storage health / doctor capability in `AC.1` / `AC.3` | Keep only if doctor remains separate. |
| `ConfigLoadRequest` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Config ingress is not part of the shared storage CRUD contract. |
| `ConfigLoadResponse` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Config ingress is not part of the shared storage CRUD contract. |
| `ConfigDoctorReport` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Config doctor is not part of the shared storage CRUD contract. |
| `InboxSourceFileRecord` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude inbox file discovery detail. |
| `InboxIngressImportRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressImportResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressIdentityFingerprintRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressIdentityFingerprintResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressDiagnosticsRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressDiagnosticsResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose inbox import wrappers. |
| `InboxIngressRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Envelope wrapper family must disappear. |
| `InboxIngressResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Envelope wrapper family must disappear. |
| `InboxExportRecordRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `InboxExportRecordResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `InboxExportReexportMessageRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `InboxExportReexportMessageResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `ClaudeCompatibilityDeliveryMode` | enum | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Compatibility delivery policy is Claude-backend-only. |
| `InboxExportAppendMessageSetRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `InboxExportAppendMessageSetResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Shared contract must not expose export wrappers. |
| `InboxExportRequest` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Envelope wrapper family must disappear. |
| `InboxExportResponse` | struct | `delete-wrapper` | `internalize-claude` | move or delete in `AC.2` / `AC.6` | Envelope wrapper family must disappear. |
| `NonClaudeOutboundDeliveryRequest` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Outbound delivery seam is not part of the shared storage CRUD contract. |
| `NonClaudeOutboundDeliveryResponse` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Outbound delivery seam is not part of the shared storage CRUD contract. |
| `TaskStore` | trait | `replace-trait` | `replace-and-delete` | `atm-storage::TaskStore` in `AC.1` | Old trait deleted when shared contract lands. |
| `TaskStoreDoctor` | trait | `replace-trait` | `capability-review` | health / doctor capability in `AC.1` / `AC.3` | Must not survive unchanged into `atm-storage`. |
| `RosterStore` | trait | `replace-trait` | `replace-and-delete` | `atm-storage::RosterStore` in `AC.1` | Old trait deleted when shared contract lands. |
| `RosterStoreDoctor` | trait | `replace-trait` | `capability-review` | health / doctor capability in `AC.1` / `AC.3` | Must not survive unchanged into `atm-storage`. |
| `ConfigIngress` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Config seam remains outside shared storage contract. |
| `ConfigDoctor` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Config seam remains outside shared storage contract. |
| `InboxIngress` | trait | `replace-trait` | `internalize-claude` | backend-only import seam review in `AC.2` / `AC.6` | Must not survive as a shared storage trait. |
| `InboxExport` | trait | `replace-trait` | `internalize-claude` | backend-only export seam review in `AC.2` / `AC.6` | Must not survive as a shared storage trait. |
| `NonClaudeOutbound` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` / `AC.6` | Outbound delivery seam remains outside shared storage CRUD contract. |

## `crates/atm-core/src/boundary/runtime.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `RemoteReplayStateRecord` | struct | `capability-candidate` | `capability-review` | replay capability review in `AC.1` / `AC.3` | Replay state belongs below the shared CRUD core unless explicitly promoted. |
| `RuntimeBundle` | struct | `delete-bundle` | `replace-and-delete` | deleted in `AC.4` / `AC.6` | Backend-shaped runtime assembly bundle must not survive. |
| `RemoteReplayStore` | trait | `replace-trait` | `capability-review` | replay capability review in `AC.1` / `AC.3` | Shared capability only if justified. |
| `RuntimeStorageFinalizer` | trait | `replace-trait` | `capability-review` | finalizer / lifecycle capability review in `AC.3` / `AC.4` | Must not remain an `atm-core`-owned backend seam. |

## Decisive Internal Seam

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `delivery_execution::ClaudeInboxWriter` | `pub(crate)` trait | `replace-trait` | `internalize-claude` | move below `atm-storage-claude` in `AC.2` / `AC.4` | Key proof that Claude storage is still an ad hoc `atm-core` seam instead of a backend crate. |

## Public `atm-rusqlite` Support Types

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `SqliteWriterLockGuard` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite implementation detail, not shared contract. |
| `SqliteBoundaryAssembly` | struct | `delete-bundle` | `replace-and-delete` | deleted or replaced in `AC.3` / `AC.4` | Backend-shaped assembly helper must not survive above trait line. |
| `SqliteObservabilityOutcome` | enum | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite observability detail. |
| `SqliteObservabilityEvent` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite observability detail. |
| `SqliteObservability` | trait | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite observability detail. |
| `NullSqliteObservability` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite observability detail. |

## Supporting Canonical Seed Types Already Present

These are not part of the searched public-surface census above, but they are
already explicit convergence anchors and must be reused rather than cloned:

- `schema::MessageEnvelope` -> converges into canonical shared `Message`
- `schema::AtmMessageId` -> remains the underlying message identity; `MessageKey`
  must wrap it per `ADR-012`
- `TeamName` -> reused in the shared contract
- `AgentName` -> reused in the shared contract
- `TaskId` -> reused in the shared contract

## Required Use In Later Sprints

- `AC.1` uses this ledger as the source of truth for which mail / task /
  roster types survive into `atm-storage`
- `AC.2` uses this ledger to keep Claude-only projections and import/export
  mechanics below the backend trait line
- `AC.3` uses this ledger to keep SQLite-only mechanics below the backend trait
  line
- `AC.4` uses this ledger to delete backend-shaped seams still owned by
  `atm-core`
- `AC.6` uses this ledger as the final type-deletion checklist
