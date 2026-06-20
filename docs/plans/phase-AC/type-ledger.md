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

Capability review rule:

- `capability-candidate` is fail-closed
- unless a later sprint explicitly promotes the type family into a named
  capability trait with a documented justification, the default outcome is
  deletion or backend-internalization
- unresolved capability-candidate rows are not allowed to roll past their
  owning sprint as an open-ended bucket

Planning rule:

- every type family has exactly one primary closure sprint
- later sprints may perform consumer cutover or verification, but they do not
  become co-owners of the same type decision
- `AC.6` is verification and residual deletion closeout, not a shadow owner of
  work that should have closed in `AC.1` through `AC.5`

Final action shorthand used throughout the ledger:

- `move-to-atm-storage` — becomes part of the shared `atm-storage` contract
- `merge-and-delete` — merged into a canonical shared type, old concrete type deleted
- `replace-and-delete` — old trait or seam replaced, old type deleted
- `internalize-claude` — move below `atm-storage-claude` as backend-only detail
- `internalize-rusqlite` — move below `atm-storage-rusqlite` as backend-only detail
- `retain-outside-storage` — remains in the repo but stays outside the storage contract
- `capability-review` — only survives if later sprint explicitly keeps it as a small capability type
- `capability-review` rows default to delete-or-internalize; promotion to a
  named capability requires an explicit sprint-level keep decision
- `delete-speculative` — speculative surface removed by default; temporary
  quarantine is allowed only when the owning sprint records a concrete blocker
  that prevents immediate deletion

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

## Single-Closure Ownership Policy

Default primary ownership by final-action family:

- `move-to-atm-storage`
  - primary closure sprint: `AC.1`
  - later use: `AC.5` may converge remaining RPC/body consumers onto the
    canonical shared type, but it does not redefine the type
- `merge-and-delete`
  - primary closure sprint: `AC.1` for shared-contract families
  - later use: `AC.5` may finish transport/body usage convergence; `AC.6`
    verifies deletion only
- `internalize-claude`
  - primary closure sprint: `AC.2`
  - later use: `AC.4` cuts remaining core consumers over; `AC.6` verifies that
    no shared/public leakage remains
- `internalize-rusqlite`
  - primary closure sprint: `AC.3`
  - later use: `AC.4` cuts remaining core consumers over when needed; `AC.6`
    verifies that no shared/public leakage remains
- `replace-and-delete`
  - primary closure sprint:
    - `AC.1` when replacing shared storage traits
    - `AC.3` when replacing SQLite-owned backend bundles
    - `AC.4` when replacing core-owned backend seams
- `capability-review`
  - primary closure sprint: `AC.3`
  - `AC.1` may set caps or candidate names, but final keep/delete/internalize
    decisions close in `AC.3`
- `retain-outside-storage`
  - primary closure sprint: `AC.4`
  - `AC.6` verifies docs/code no longer misclassify those seams as storage

Interpretation rule:

- if a table row still names more than one sprint in its target text, the first
  sprint is not automatically the owner
- the family policy in this section controls unless the exceptions table below
  says otherwise

## Single-Closure Exceptions

These rows need explicit primary ownership because their lifecycle spans more
than one sprint in a non-default way.

| Type / Family | Primary Closure Sprint | Later Sprint Role | Why |
| --- | --- | --- | --- |
| `MailStoreMessageRecord` | `AC.1` | `AC.5` usage convergence only | Canonical `Message` is defined in `AC.1`; transport/body consumers finish migrating in `AC.5`. |
| `TaskStoreTaskRecord` | `AC.6` | no later AC owner | Speculative task-store surface is deleted in cleanup rather than converged into the initial shared contract. |
| `MailStoreMailboxMetadataRow` | `AC.1` | `AC.5` query/body convergence only | The replacement query helper shape is chosen in `AC.1`; usage cleanup lands later. |
| `ReplaySource` / replay candidate rows | `AC.3` | `AC.1` contract cap only | Whether replay survives as a capability or backend-internal concern closes with the backend convergence sprint. |
| doctor / health candidate rows | `AC.3` | `AC.1` contract cap only | Capability keep/delete/internalize decision depends on concrete backend convergence, not only naming. |
| `delivery_execution::ProjectionMailboxWriter` | `AC.2` | `AC.4` consumer cutover only | The seam moves below `atm-storage-claude` in `AC.2`; `AC.4` only removes remaining core usage. |
| `SqliteBoundaryAssembly` | `AC.3` | `AC.4` consumer cutover only | The backend assembly replacement is a SQLite convergence decision before core cleanup consumes it. |
| `RuntimeBundle` | `AC.4` | `AC.6` verification only | This is a core-owned backend seam; cleanup verification is not primary ownership. |
| `Config*` retain-outside rows | `AC.4` | `AC.6` verification only | `AC.4` classifies them as non-storage seams; `AC.6` checks docs/code drift only. |
| `SourceIngress*` / `ProjectionExport*` rows | `AC.2` | `AC.6` verification only | Claude-backend internalization closes in `AC.2`; later grep/delete is only proof. |

## AC.6 Closure Notes

The AC.6 branch closes the remaining cleanup families this way:

- speculative `TaskStore*` types and `TaskStore` / `TaskStoreDoctor` are
  deleted from `atm-core`, and the last runtime/daemon compile-bridge usage is
  removed instead of quarantined
- the old Claude `SourceIngress*` / `ProjectionExport*` public wrapper and
  trait families are deleted from the shared seam; daemon consumers use direct
  `atm-storage-claude::compat` functions and canonical `SourceFileRecord`
- `SqliteObservability*` leaves `atm-storage` entirely and is owned by
  `atm-storage-rusqlite`
- no `quarantine-reason` rows were needed for AC.6

## `crates/atm-core/src/boundary/mod.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `MessageKey` | struct | `retain-shared` | `move-to-atm-storage` | `atm-storage::MessageKey` in `AC.1` | Must wrap `AtmMessageId` per `ADR-012`. |
| `TaskState` | struct | `retain-shared` | `move-to-atm-storage` | task state newtype / enum in `AC.1` | Keep as semantic state, not backend-shaped wrapper. |
| `AckTransition` | struct | `retain-shared` | `move-to-atm-storage` | shared ack-transition helper in `AC.1` | Shared semantic helper, not backend-specific. |
| `AtmProtocol` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | RPC / protocol boundary, not storage. |
| `ClientTransport` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Transport boundary only. |
| `ServerTransport` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Transport boundary only. |
| `RequestDispatcher` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | RPC dispatch, not storage. |
| `AdvisoryStreamSink` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Advisory stream behavior is not storage CRUD. |
| `NotificationSink` | trait | `out-of-scope-transport` | `retain-outside-storage` | compare against `StorageNotifier` in `AC.4` | Must not be silently reused as the storage notifier without review. |
| `StatusSource` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Runtime status surface, not storage. |
| `WatchEventSource` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Watch surface, not storage. |
| `ReconcileCoordinator` | trait | `out-of-scope-transport` | `retain-outside-storage` | classified in `AC.4` | Reconcile workflow, not storage. |

## `crates/atm-core/src/boundary/mail.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `ReplaySource` | struct | `capability-candidate` | `capability-review` | replay capability review in `AC.3` | Replay is not part of the core CRUD contract by default; `AC.1` only caps the shared contract surface. |
| `MailStoreMessageRecord` | struct | `merge-into-shared` | `merge-and-delete` | canonical `Message` in `AC.1` | Main storage message record to collapse; `AC.5` only migrates remaining RPC/body consumers. |
| `MailMessageState` | struct | `merge-into-shared` | `merge-and-delete` | shared message-state helper in `AC.1` | Must not remain a separate backend-shaped record. |
| `MessageFingerprint` | struct | `retain-shared` | `move-to-atm-storage` | shared helper / newtype in `AC.1` | Candidate cross-backend helper if still needed. |
| `MailStoreIngestReplayState` | struct | `capability-candidate` | `capability-review` | replay capability in `AC.3` | Keep out of base CRUD contract unless justified; `AC.1` only caps the shared contract surface. |
| `MailStoreHealthSnapshot` | struct | `capability-candidate` | `capability-review` | storage health capability in `AC.3` | Health / doctor surface, not CRUD core; `AC.1` only caps the shared contract surface. |
| `MailStoreMailboxMetadataRow` | struct | `merge-into-shared` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: still consumed by retained mailbox paths in `ack/mod.rs`, `read/mod.rs`, `clear/mod.rs`, `list.rs`, and `service_runtime_store.rs`; full deletion moves to phase-AD after the final retained-surface cutover. |
| `MailStoreQueryMailboxMetadataRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper family collapse; `AC.6` only verifies no stragglers survived. |
| `MailStoreQueryMailboxMetadataResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper family collapse; `AC.6` only verifies no stragglers survived. |
| `MailStoreMailboxMetadataCounts` | struct | `merge-into-shared` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: survives as part of the same retained mailbox compile-bridge family anchored in `service_runtime_store.rs`; full deletion moves to phase-AD after the final retained-surface cutover. |
| `MailStoreQueryMailboxMetadataCountsRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper family collapse; `AC.6` only verifies no stragglers survived. |
| `MailStoreQueryMailboxMetadataCountsResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper family collapse; `AC.6` only verifies no stragglers survived. |
| `MailStoreBootstrapRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.6` | Backend bootstrap did not survive the closeout sweep and is no longer part of the shared storage DTO surface. |
| `MailStoreBootstrapResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.6` | Backend bootstrap did not survive the closeout sweep and is no longer part of the shared storage DTO surface. |
| `MailStoreTransactionRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted or replaced by capability in `AC.1` | No RPC-style transaction wrapper in base storage contract; `AC.6` only verifies no stragglers survived. |
| `MailStoreTransactionResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted or replaced by capability in `AC.1` | No RPC-style transaction wrapper in base storage contract; `AC.6` only verifies no stragglers survived. |
| `MailStoreUpsertMessageRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `save_message` absorbs this operation; `AC.6` only verifies no stragglers survived. |
| `MailStoreUpsertMessageResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `save_message` absorbs this operation; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadMessageRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `load_message` absorbs this operation; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadMessageResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `load_message` absorbs this operation; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadStoredMessageRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Stored-message distinction must be re-justified or removed; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadStoredMessageResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Stored-message distinction must be re-justified or removed; `AC.6` only verifies no stragglers survived. |
| `UpsertMailMessageStateRequest` | struct | `delete-wrapper` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: the request/response DTO state-mutation family still survives through the retained compile-bridge used by `service_runtime_store.rs`; request/response DTO pattern is acknowledged tech debt and deletes in phase-AD. |
| `UpsertMailMessageStateResponse` | struct | `delete-wrapper` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: the request/response DTO state-mutation family still survives through the retained compile-bridge used by `service_runtime_store.rs`; request/response DTO pattern is acknowledged tech debt and deletes in phase-AD. |
| `LoadMailMessageStateRequest` | struct | `delete-wrapper` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: the request/response DTO state-lookup family still survives through the retained compile-bridge used by `service_runtime_store.rs`; request/response DTO pattern is acknowledged tech debt and deletes in phase-AD. |
| `LoadMailMessageStateResponse` | struct | `delete-wrapper` | `merge-and-delete` | phase-AD quarantine | quarantine-reason: the request/response DTO state-lookup family still survives through the retained compile-bridge used by `service_runtime_store.rs`; request/response DTO pattern is acknowledged tech debt and deletes in phase-AD. |
| `MailStoreRecordIngestReplayStateRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Replay capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreRecordIngestReplayStateResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Replay capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadIngestReplayStateRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Replay capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreLoadIngestReplayStateResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Replay capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreHealthSnapshotRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Health capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreHealthSnapshotResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Health capability must not use RPC-style wrappers; `AC.6` only verifies no stragglers survived. |
| `MailStoreRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Envelope wrapper family must disappear; `AC.6` only verifies no stragglers survived. |
| `MailStoreResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Envelope wrapper family must disappear; `AC.6` only verifies no stragglers survived. |
| `MailStoreDoctorReport` | struct | `capability-candidate` | `capability-review` | storage health / doctor capability in `AC.3` | Keep only if doctor remains a separate capability shape; `AC.1` only caps the shared contract surface. |
| `MailStore` | trait | `replace-trait` | `MessageStore` in `AC.1` | Old trait deleted when shared contract lands. |
| `MailStoreDoctor` | trait | `replace-trait` | `capability-review` | health / doctor capability in `AC.3` | Must not survive unchanged into `atm-storage`; `AC.1` only caps capability count and naming. |

## `crates/atm-core/src/boundary/store.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `TaskStoreTaskMetadata` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` instead of being preserved as a speculative compatibility surface. |
| `TaskStoreTaskRecord` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` instead of being preserved as a speculative compatibility surface. |
| `RosterStoreHealthSnapshot` | struct | `capability-candidate` | `capability-review` | storage health capability in `AC.3` | Not part of CRUD core; `AC.1` only caps the shared contract surface. |
| `RosterMemberKind` | enum | `retain-shared` | `move-to-atm-storage` | shared enum in `AC.1` | Semantic roster member property. |
| `RosterHarness` | enum | `retain-shared` | `move-to-atm-storage` | shared enum in `AC.1` | Semantic roster harness property. |
| `RosterMemberRecord` | struct | `merge-into-shared` | `merge-and-delete` | canonical `RosterMember` in `AC.1` | Main roster member record to collapse. |
| `ProjectionRosterMember` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude projection type, not shared contract. |
| `ProjectionRoster` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude projection type, not shared contract. |
| `TaskStoreCreateTaskRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreCreateTaskResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreLoadTaskRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreLoadTaskResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreUpdateTaskRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreUpdateTaskResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreAttachMessageLinkRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreAttachMessageLinkResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreDetachMessageLinkRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreDetachMessageLinkResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreRecordAckTransitionRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreRecordAckTransitionResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted from `atm-core` with the rest of the speculative task wrapper family. |
| `TaskStoreQueryTaskMetadataRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted instead of serving as a seed shape for future task storage. |
| `TaskStoreQueryTaskMetadataResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted instead of serving as a seed shape for future task storage. |
| `TaskStoreRequest` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted with the speculative task envelope family. |
| `TaskStoreResponse` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted with the speculative task envelope family. |
| `TaskStoreDoctorReport` | struct | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted with the speculative task doctor surface. |
| `RosterStoreReplaceRosterRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper collapse; `AC.6` only verifies no stragglers survived. |
| `RosterStoreReplaceRosterResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper collapse; `AC.6` only verifies no stragglers survived. |
| `RosterStoreLoadRosterRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper collapse; `AC.6` only verifies no stragglers survived. |
| `RosterStoreLoadRosterResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Wrapper collapse; `AC.6` only verifies no stragglers survived. |
| `RosterStoreQueryMembershipRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Query semantics must collapse into shared roster query helpers or disappear; `AC.6` only verifies no stragglers survived. |
| `RosterStoreQueryMembershipResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Query semantics must collapse into shared roster query helpers or disappear; `AC.6` only verifies no stragglers survived. |
| `RosterStoreHealthSnapshotRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Health capability must not use wrapper DTOs; `AC.6` only verifies no stragglers survived. |
| `RosterStoreHealthSnapshotResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Health capability must not use wrapper DTOs; `AC.6` only verifies no stragglers survived. |
| `RosterStoreListTeamsRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `list_teams` method should not need a request DTO; `AC.6` only verifies no stragglers survived. |
| `RosterStoreListTeamsResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | `list_teams` method should not need a response DTO; `AC.6` only verifies no stragglers survived. |
| `RosterStoreRequest` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Envelope wrapper family must disappear; `AC.6` only verifies no stragglers survived. |
| `RosterStoreResponse` | struct | `delete-wrapper` | `merge-and-delete` | deleted in `AC.1` | Envelope wrapper family must disappear; `AC.6` only verifies no stragglers survived. |
| `RosterStoreDoctorReport` | struct | `capability-candidate` | `capability-review` | storage health / doctor capability in `AC.3` | Keep only if doctor remains separate; `AC.1` only caps the shared contract surface. |
| `ConfigLoadRequest` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Config ingress is not part of the shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |
| `ConfigLoadResponse` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Config ingress is not part of the shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |
| `ConfigDoctorReport` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Config doctor is not part of the shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |
| `SourceFileRecord` | struct | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Claude inbox file discovery detail. |
| `SourceImportRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Request` wrapper family. |
| `SourceImportResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Response` wrapper family. |
| `SourceIdentityFingerprintRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Request` wrapper family. |
| `SourceIdentityFingerprintResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Response` wrapper family. |
| `SourceDiagnosticsRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Request` wrapper family. |
| `SourceDiagnosticsResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `SourceIngress*Response` wrapper family. |
| `SourceIngressRequest` | struct | `delete-wrapper` | `delete-and-rename` | deleted in `AC.6` | Unused envelope wrapper deleted during cleanup. |
| `SourceIngressResponse` | struct | `delete-wrapper` | `delete-and-rename` | deleted in `AC.6` | Unused envelope wrapper deleted during cleanup. |
| `ProjectionRecordRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Request` wrapper family. |
| `ProjectionRecordResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Response` wrapper family. |
| `ProjectionReexportMessageRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Request` wrapper family. |
| `ProjectionReexportMessageResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Response` wrapper family. |
| `ProjectionAppendMode` | enum | `backend-only` | `internalize-claude` | `atm-storage-claude` in `AC.2` | Compatibility delivery policy is Claude-backend-only. |
| `ProjectionAppendMessageSetRequest` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Request` wrapper family. |
| `ProjectionAppendMessageSetResponse` | struct | `delete-wrapper` | `internalize-claude` | internalize in `AC.2`; naming closeout in `AC.6` | Shared contract no longer exposes the old `ProjectionExport*Response` wrapper family. |
| `ProjectionExportRequest` | struct | `delete-wrapper` | `delete-and-rename` | deleted in `AC.6` | Unused envelope wrapper deleted during cleanup. |
| `ProjectionExportResponse` | struct | `delete-wrapper` | `delete-and-rename` | deleted in `AC.6` | Unused envelope wrapper deleted during cleanup. |
| `NonClaudeOutboundDeliveryRequest` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Outbound delivery seam is not part of the shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |
| `NonClaudeOutboundDeliveryResponse` | struct | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Outbound delivery seam is not part of the shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |
| `TaskStore` | trait | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted instead of being normalized into `atm-storage`; future task storage starts from canonical Claude schema instead. |
| `TaskStoreDoctor` | trait | `speculative-task` | `delete-speculative` | deleted in `AC.6` | Deleted with the speculative task-store contract surface. |
| `RosterStore` | trait | `replace-trait` | `replace-and-delete` | `atm-storage::RosterStore` in `AC.1` | Old trait deleted when shared contract lands. |
| `RosterStoreDoctor` | trait | `replace-trait` | `capability-review` | health / doctor capability in `AC.3` | Must not survive unchanged into `atm-storage`; `AC.1` only caps the shared contract surface. |
| `ConfigIngress` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Config seam remains outside shared storage contract; `AC.6` only verifies docs/code did not drift. |
| `ConfigDoctor` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Config seam remains outside shared storage contract; `AC.6` only verifies docs/code did not drift. |
| `SourceIngress` | trait | `replace-trait` | `internalize-claude` | internalize in `AC.2` | Must not survive as a shared storage trait; `AC.6` only verifies no shared/public leakage remains. |
| `ProjectionExport` | trait | `replace-trait` | `internalize-claude` | internalize in `AC.2` | Must not survive as a shared storage trait; `AC.6` only verifies no shared/public leakage remains. |
| `NonClaudeOutbound` | trait | `out-of-scope-transport` | `retain-outside-storage` | review in `AC.4` | Outbound delivery seam remains outside shared storage CRUD contract; `AC.6` only verifies docs/code did not drift. |

## `crates/atm-core/src/boundary/runtime.rs`

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `RemoteReplayStateRecord` | struct | `capability-candidate` | `capability-review` | replay capability review in `AC.3` | Replay state belongs below the shared CRUD core unless explicitly promoted; `AC.1` only caps the shared contract surface. |
| `RuntimeBundle` | struct | `delete-bundle` | `replace-and-delete` | deleted in `AC.4` | Backend-shaped runtime assembly bundle must not survive; `AC.6` only verifies no stragglers survived. |
| `RemoteReplayStore` | trait | `replace-trait` | `capability-review` | replay capability review in `AC.3` | Shared capability only if justified; `AC.1` only caps the shared contract surface. |
| `RuntimeStorageFinalizer` | trait | `replace-trait` | `capability-review` | finalizer / lifecycle capability review in `AC.3` | Must not remain an `atm-core`-owned backend seam; `AC.4` only removes remaining consumers of the chosen replacement. |

## Decisive Internal Seam

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `delivery_execution::ProjectionMailboxWriter` | `pub(crate)` trait | `replace-trait` | `internalize-claude` | move below `atm-storage-claude` in `AC.2` | Key proof that Claude storage is still an ad hoc `atm-core` seam instead of a backend crate; `AC.4` only removes remaining consumers. |

## Public `atm-rusqlite` Support Types

| Type | Kind | Disposition | Final Action | Target / Owning Sprint | Notes |
| --- | --- | --- | --- | --- | --- |
| `SqliteWriterLockGuard` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | SQLite implementation detail, not shared contract. |
| `SqliteBoundaryAssembly` | struct | `delete-bundle` | `replace-and-delete` | deleted or replaced in `AC.3` | Backend-shaped assembly helper must not survive above trait line; `AC.4` only removes remaining consumers. |
| `SqliteObservabilityOutcome` | enum | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | Backend-owned observability seam; intentionally public only for `atm-runtime` sqlite assembly, not shared-contract export. |
| `SqliteObservabilityEvent` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | Backend-owned observability seam; intentionally public only for `atm-runtime` sqlite assembly, not shared-contract export. |
| `SqliteObservability` | trait | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | Backend-owned observability seam; intentionally public only for `atm-runtime` sqlite assembly, not shared-contract export. |
| `NullSqliteObservability` | struct | `backend-only` | `internalize-rusqlite` | `atm-storage-rusqlite` in `AC.3` | Backend-owned observability seam; intentionally public only for `atm-runtime` sqlite assembly, not shared-contract export. |

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
