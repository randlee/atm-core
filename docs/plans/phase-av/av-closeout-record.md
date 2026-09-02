# Phase AV Closeout Record

This record is the path-based inventory for the AV.3 mechanical cutover. It
is written against repository revision `db08f4591` (the AV.2 starting
revision). It distinguishes the four read-path removals/renames from the
eight synchronous control-path call sites that remain intentionally in scope
for the later `AV-FU-1` follow-up.

## AV.3 removals and renames

AV.3 must make each of these changes mechanically and gate the old shape:

| Current path and symbol | AV.3 disposition |
| --- | --- |
| `crates/atm-http-runtime/src/storage_and_nudge_router.rs` — `BlockingCoreBridge` | Rename to `ControlPathSyncBridge`; the old identifier must disappear from `crates/`. The renamed wrapper is permitted only at the residual call sites below. |
| `crates/atm-storage-rusqlite/src/writer/ops.rs` — `WriteOp::ListMessages` | Remove the pure-read writer operation; list/peek/read/doctor response data comes from the bounded reader/projection lanes. |
| `crates/atm-storage-rusqlite/src/shared_db.rs` — `SharedDb::submit_list_messages_async` | Remove the writer-backed mailbox projection submission and route the capability through `AsyncMailboxReader`. |
| `crates/atm-storage-rusqlite/src/search_reader.rs` — bespoke `SearchReader` worker loop | Remove this bespoke loop as the canonical implementation; AV.1a's bounded reader-pool type owns the search-reader instance. Existing search semantics remain unchanged. |

The corresponding composition and capability contracts are documented in
[`sprint-AV.1a-reader-lane-foundation.md`](./sprint-AV.1a-reader-lane-foundation.md)
§D1/D1a and in [`ADR-059`](../../adr/ADR-059-async-mailbox-read-concurrency.md).

## Residual AV-FU-1 bridge call sites

After AV.1b migrates the four read-family handlers, exactly these eight
`ControlPathSyncBridge::run` call sites remain by design at
`crates/atm-http-runtime/src/storage_and_nudge_router.rs`:

1. `StorageAndNudgeRouter::commit_write` — deferred-write marker
   (`prepared.mark_pending_if_deferred`), the synchronous post-admission
   marker transaction.
2. `StorageAndNudgeRouter::heartbeat` — synchronous roster validation before
   the in-memory heartbeat projection.
3. `StorageAndNudgeRouter::queue_get_next` — synchronous roster validation
   and bare-CLI FIFO drain.
4. `StorageAndNudgeRouter::graft_receiver_register` — synchronous roster
   validation and `GraftReceiverEndpointStore::register`.
5. `StorageAndNudgeRouter::graft_receiver_refresh` — synchronous roster
   validation and `GraftReceiverEndpointStore::refresh`.
6. `StorageAndNudgeRouter::graft_receiver_unregister` — synchronous roster
   validation and `GraftReceiverEndpointStore::unregister`.
7. `StorageAndNudgeRouter::graft_receiver_lookup` — synchronous roster
   validation and `GraftReceiverEndpointStore::lookup`.
8. `StorageAndNudgeRouter::clear_messages` — synchronous
   `atm_core::clear::clear_mail_with_runtime` mutation; this is not a
   read-family operation and has no writer-ingress equivalent in AV.

The eight sites are intentionally not presented as completed bridge deletion.
`AV-FU-1` owns a future async roster/member-validation port, async graft
receiver-store port, and `WriteOp::ClearMailbox` ingress. Until that work
lands, AV.3's exact-call-site architecture test MUST reject any additional
bridge use, especially a new read-family use.

## Ledger boundary

The Phase-AM closeout ledger
[`am1-removal-ledger.md`](../phase-am/am1-removal-ledger.md) is frozen. AV
adds no entries to it and does not edit it. This AV-owned record is the sole
place for the AV.3 residual inventory.
