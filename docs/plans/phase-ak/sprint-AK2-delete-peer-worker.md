---
title: AK.2 Delete daemon peer worker
status: proposed
branch: feature/pak-s2-delete-peer-worker
worktree: ../atm-core-worktrees/feature/pak-s2-delete-peer-worker
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.1
parallel_safe: false
---

# AK.2 — delete daemon peer worker

## Closure

Delete the daemon-owned host-qualified delivery worker before its replacement.
This sprint deletes queue/thread/reload machinery and the legacy custom TLS
module only. AK.6 preserves its interop-only subset from the pre-AK.2 baseline.
A host-qualified
origin record retains exactly one immutable outbound write and destination host
(`peerOutbound`) as durable message data; it is not a queue or worker state.
Host-qualified writes are not delivered until AK.4.

## Deliverables

1. Delete `peer_drain_coordinator.rs`, `PeerDeliveryCoordinator`, worker
   creation/join/shutdown, `PeerJob`, `PeerWork`, channels, per-message
   threads, `https_transport.rs`, and worker-only tests.
2. Delete `PostCommitWorkKey::PeerDelivery` and its router signal. Retain the
   existing local post-write nudge path.
3. Delete worker-only retry/recovery policy and observability. Do not replace
   them with renamed queues, task handles, threads, background scans, or an
   immediate SQLite reload.
4. Retain `peerOutbound` only as the immutable persisted write plus destination
   host. It starts no work, owns no retry state, and is the sole durable input
   for AK.3 canonicalization and AK.5's later timer backlog. Do not create a
   second outbox table or duplicate payload representation.
5. Mark only the worker/replay portion of `REQ-CORE-TRANSPORT-003B` and
   ADR-038 superseded by AK.2. Update Phase AI status text, architecture,
   boundaries, and project plan so none promise daemon worker delivery or
   replay. Do not define resend-cache semantics (AK.5 owns them), revise
   alias semantics (AK.3 owns them), or revise active direct delivery (AK.4
   owns it).
6. Update `boundaries/atm-storage/peer-config-store.toml` in this same PR:
   remove `PeerSyncPolicy` from its ownership and contract request/response
   types, and remove worker-policy wording. Retain only configuration that
   survives AK.2; do not add an AK.5 resend-cache contract early.

## Literal deletion ledger

This ledger is the AK.2 implementation scope. `Delete` means remove the named
item and all tests that prove its retired behavior. `Rewrite` means retain only
the listed local-nudge or durable-data responsibility. `Defer` means AK.2 does
not change it.

The literal ledger is also AK.2's complete inventory of important structs,
enums, traits, constants, storage methods, routes, and worker boundaries.
AK.2 may not introduce a replacement type or execution mechanism unless the
ledger names it explicitly.

### Compiler-attributed pre-delete map

AK.2 begins with a marker-only commit: apply `#[deprecated(note = "AK.2 …")]`
to every listed production deletion/rename target before deleting any of them.
The markers are intentional discovery tools, not a compatibility policy or
suppression: remove blanket `allow(deprecated)` from `atm-daemon` and
`atm-storage-rusqlite` for this checkpoint. Record the marker-only
`cargo check --workspace --all-targets` output as the complete compiler map of
target references; every warning must belong to a row below. Then delete or
rewrite the marked surfaces and finish with no AK.2 deprecation warnings.

| Compiler-attributed caller | AK.2 action | Replacement, if any |
| --- | --- | --- |
| `peer_drain_coordinator.rs`, `peer_delivery_observability.rs` | Delete whole modules | None. |
| `runtime_health.rs` coordinator fields/build/start/stop, projection, PeerSync dispatch | Delete | `start_local_post_write_executor` / `stop_local_post_write_executor` start and stop only the retained local nudge executor. |
| `runtime_health/peer_delivery_router.rs` peer event and `PeerDelivery` signal | Rewrite | Host-qualified origin returns after persistence; peer receipts and hostless writes use `signal_local_post_write`. |
| `runtime_health/post_commit_work.rs::PeerPostCommitWorkQueue` | Rename and rewrite | `LocalPostCommitWorkQueue`; no coordinator field or peer arm. |
| `composition.rs` custom TLS listener/client lifecycle and outbound `https_transport` slot | Delete | AK.2 leaves no legacy transport lifecycle. AK.4 creates the minimal plain receiver; AK.6 preserves only its baseline interop fixture. |
| `atm-core::api`, `protocol`, local-IPC request classification, transport test adapter | Delete PeerSync route/envelopes/arms/tests | None. |
| `atm::commands::peer::{sync,sync-policy}` and `CliComposition::peer_sync` | Delete | None. Provisioning records and configuration display remain only as AK.6 interop-fixture input; they must not install or select an active transport after AK.2. |
| `PeerSyncPolicy`, `PeerConfigStore` policy methods, SQLite adapter/table/index | Delete | None. AK.5 creates a distinct resend-cache setting only after AK.4 works. |
| doctor `PeerLink*` projection and CLI `configured_peer_links` | Delete | Retain configured-peer report only; it must not invent delivery health. |

No marker is placed on the retained `peerOutbound` record,
`OutboundMessageQuery::page_for_peer`, `PostCommitWorkQueue`, local nudge
delivery, peer configuration, or HTTPS receiver. They are not worker state.

### 1. Delete the peer delivery worker module

Delete `crates/atm-daemon/src/peer_drain_coordinator.rs` in full. It owns the
old state machine and no item in it survives:

| Item | Action | Why |
| --- | --- | --- |
| `POST_COMMIT_QUEUE_DEPTH`, `MAX_ACTIVE_PEER_JOBS`, `MAX_ACTIVE_PEER_JOBS_PER_HOST`, `PEER_DELIVERY_WORKER_DEADLINE`, `PEER_SYNC_REQUEST_DEADLINE`, `PEER_DRAIN_JOIN_POLICY`, `PEER_JOB_JOIN_POLICY` | Delete | Capacity, deadline, and join policy exist only for the retired worker. |
| `PeerSyncOutcome` | Delete | Its only consumer is the explicit worker reconciliation API. |
| `PeerDeliveryCoordinator` and `signal_after_persist`, `sync_peer`, `start`, `stop` | Delete | This trait is the coordinator boundary AK.2 removes. |
| `PeerJob`, `EligiblePeerWrite`, `PeerWork`, `JobDeliveryResult`, `JobState` | Delete | Per-message job, queue, eligibility, and in-flight state are the prohibited state machine. |
| `JobState::{try_take, release}` | Delete | Per-host/global job admission disappears with `PeerJob`. |
| `PeerDrainCoordinator` | Delete | The coordinator has no replacement in AK.2. |
| `PeerDrainCoordinator::{new, record, take_job, release_job, run_job, eligible_request, deliver_one, run, reap_finished_workers}` | Delete | These methods respectively construct, observe, admit, recover, reload, deliver, dispatch, and reap the prohibited peer work. |
| `impl PeerDeliveryCoordinator for PeerDrainCoordinator::{signal_after_persist, sync_peer, start, stop}` | Delete | No peer scheduler remains. |
| `decode_request` | Delete | AK.2 never reloads a persisted request. AK.5 will decode its durable backlog through a new, explicitly scoped reader if it still needs one. |
| Test helper `job` and every coordinator unit test | Delete | They assert coalescing, caps, isolated threads, and worker reaping—the retired behavior. |

### 2. Delete only the peer half of post-commit work

`crates/atm-daemon/src/runtime_health/post_commit_work.rs` is **not** deleted:
it remains the bounded local nudge executor. Rename
`PeerPostCommitWorkQueue` to `LocalPostCommitWorkQueue` to prevent the old
role from surviving in the name.

| Item | Action | Why |
| --- | --- | --- |
| `PostCommitWorkKey::PeerDelivery { peer, message_id }` | Delete | It is the sole post-commit handoff into the peer coordinator. |
| `PostCommitWorkKey::LocalNudge` | Retain | It drives the ordinary local/received-message nudge path. |
| `PostCommitWorkQueue::signal` | Retain | It remains the local-nudge boundary only. |
| `PeerPostCommitWorkQueue` | Rewrite/rename | Mark as an AK.2 rename target in the marker-only commit. Become `LocalPostCommitWorkQueue`; delete its `coordinator` field. |
| `LocalPostCommitWorkQueue::new` | Rewrite | Remove the coordinator parameter and all peer construction. |
| `register_local_nudge`, `start`, `stop`, `run`, `signal`, `remove_local_nudge_target` | Retain/rewrite | Preserve local nudge work; `signal` accepts only `LocalNudge`; `run` has no peer arm. |
| `PostCommitNudgeTarget`, `DaemonGraftPostSendPort`, `DaemonGraftPostSendPort::new`, `deliver_post_send`, `deliver_post_send_to_graft_receiver`, `graft_transport_error`, `graft_recipient_unavailable_error` | Retain | These implement the one ordinary receiver/local nudge path, not peer delivery. |

The remaining local-nudge thread is not a peer coordinator, retry worker, DNS
worker, or per-message sender. It is unchanged post-write notification work.

### 3. Rewrite dispatcher routing; delete worker composition

| Location/item | Action | Exact AK.2 result |
| --- | --- | --- |
| `runtime_health/peer_delivery_router.rs::PostWriteRouter::dispatch` | Rewrite | Peer receipts and hostless writes call `signal_local_post_write`. A host-qualified origin write returns after persistence: no event, queue signal, SQLite reload, DNS, socket, TLS, or local nudge. |
| `signal_local_post_write` | Retain | It is the common ordinary nudge path for local writes and received peer writes. |
| `DaemonRequestDispatcher::peer_delivery_coordinator`, `post_commit_work_queue`, `peer_delivery_projection` fields | Delete | All are worker/projection ownership. Replace the peer lifecycle methods with `start_local_post_write_executor` / `stop_local_post_write_executor`, each delegating only to `LocalPostCommitWorkQueue`. |
| `build_peer_delivery_coordinator` | Delete | Constructs the retired state machine. |
| dispatcher construction in `new` and `new_for_test` | Rewrite | Construct one `LocalPostCommitWorkQueue`; do not request `outbound_message_query` or build peer state. |
| `start_peer_drain_coordinator`, `stop_peer_drain_coordinator` | Delete | Lifecycle belongs to the deleted worker. |
| `record_peer_delivery_event`, `peer_link_statuses`, `sync_peer` | Delete | All expose worker/projection behavior. |
| `RequestEnvelope::PeerSync` dispatch arm | Delete | There is no explicit worker reconciliation in AK.2. |
| `composition::{start_https_listeners, stop_https_listeners}` and their worker calls | Delete | The entire legacy custom transport lifecycle disappears; AK.4 owns the replacement plain receiver. |
| `DaemonRequestDispatcher::{https_transport, install_https_transport, clear_https_transport}` and `RuntimeComposition::https_transport` | Delete | These slots exist only for the legacy custom transport; they have no AK.2 replacement. |
| `atm-daemon/src/lib.rs` modules `peer_drain_coordinator`, `peer_delivery_observability`, `https_transport` | Delete | All three modules disappear. `peer_resolution` remains only until AK.3 replaces it with configured alias normalization. |

### 4. Delete peer reconciliation API and policy, retain durable writes

| Item | Action | Why |
| --- | --- | --- |
| `RequestEnvelope::PeerSync`, `ResponseEnvelope::PeerSync`, `PeerSyncRequest`, `PeerSyncOutcome`, `PeerSyncDisposition` | Delete | Public protocol only triggers the retired worker. |
| `api.rs` `PEER_SYNC_PREFIX`, `HttpRouteKind::PeerSync`, route spec, encode/decode arms, `peer_sync_path_host`, `ApiRequest::PeerSync`, peer-sync API tests | Delete | Remove the worker-only HTTP resource; do not replace it in AK.2. |
| `atm/src/composition.rs::peer_sync` and request classification | Delete | CLI no longer invokes a daemon worker. |
| `atm/src/commands/peer.rs` `PeerSubcommand::Sync`, `SyncPolicyCommand`, `SyncPolicySubcommand`, `run_sync`, `parse_whole_seconds`, sync-policy dispatch/tests | Delete | CLI controls only the retired reconciliation policy. Provisioning/configuration commands may remain solely for AK.6's isolated fixture, never active daemon transport. |
| `PeerSyncPolicy`, `MAX_PEER_SYNC_BATCH_MESSAGES`, `MAX_PEER_SYNC_MESSAGE_AGE`, `PeerSyncPolicy::{validate, default}`, `duration_seconds` | Delete | Configuration exists only for worker scanning. |
| `PeerConfigStore::{peer_sync_policy, save_peer_sync_policy}` and re-export | Delete | No durable recovery policy remains. |
| SQLite policy methods/tests/imports | Delete | Remove the dead persistence adapter surface. |
| `peer_sync_policies` table and `idx_peer_sync_policies_host` | Delete | AK.2 explicitly drops this obsolete configuration from existing databases; do not leave unused durable state. |
| `OutboundMessageQuery::find_for_peer` and its SQLite implementation/test | Delete | It exists only for a coordinator's immediate post-persist reload. |
| `StoredPeerWrite`, `OutboundMessageQuery::page_for_peer`, SQLite page query | Retain | These are durable immutable message data and the future AK.5 backlog reader; AK.2 does not schedule or read them. |
| `peerOutbound` envelope value and helpers | Retain | One canonical immutable write plus destination host. It is data, never a queue, retry record, or worker signal. |

### 5. Delete worker-only doctor projection

Delete `crates/atm-daemon/src/peer_delivery_observability.rs` in full:
`PeerDeliveryEventKind`, `PeerDeliveryEvent`, `PeerDeliveryProjection`,
`PeerDeliveryProjectionState`, `ProjectedPeerLinkStatus`,
`PeerDeliveryProjection::{record, statuses, project}`, `emit_retained_event`,
`apply_event_to_status`, `peer_link_quality_for_error`, and their tests.

Delete the corresponding doctor model, because it reports a worker state that
will no longer exist: `PeerLinkQuality`, `PeerDrainState`, `PeerLinkStatus`,
`PeerLinkStatus::misconfigured`, and `DaemonRuntimeDoctorReport::peer_links`.
Delete `atm::commands::doctor::configured_peer_links` and its tests. Retain
`peer_config` reporting only; it reports configured data rather than invented
delivery health.

### 6. Remove tests, contracts, and historical promises

| Surface | Action |
| --- | --- |
| `crates/atm-daemon/src/tests/runtime_root/peer_reconciliation.rs` | Delete whole file: every assertion is peer scan/replay behavior. |
| `crates/atm-daemon/src/tests/runtime_root/peer_observability.rs` | Delete whole file: every assertion is the deleted projection. |
| `runtime_root/local_ipc.rs`, `runtime_root.rs`, `peer_failure.rs` | Rewrite the blocked-worker/recovery tests into host-qualified admission proofs: one persisted immutable record, no peer attempt, no local nudge, prompt local response. Delete `BlockingPeerDelivery`, `RouteFailure`, worker start/stop, and policy setup. |
| `local_ipc_transport/request_worker.rs`, `atm-core/src/transport/testing.rs` | Delete `PeerSync` request arms and tests. |
| `docs/atm-daemon/openapi.yaml`, `crates/atm/tests/openapi_surface_baseline.json` | Delete peer-sync route/schema/baseline entries and regenerate the accepted API surface. |
| `crates/atm-architecture/tests/boundary_enforcement.rs` | Delete tests requiring `peer_drain_coordinator.rs` or `PostCommitWorkKey::PeerDelivery`; replace with gates proving those identifiers are absent and host-qualified admission has no post-commit peer signal. Keep unrelated host-loopback guards. |
| Worker/replay portion of `REQ-CORE-TRANSPORT-003B`, ADR-038, Phase AI worker wording in requirements/boundaries/project plan | Mark superseded by AK.2; do not rewrite historical sprint evidence as though it never existed. AK.5 exclusively defines later resend-cache semantics for `-003B`. |

### 7. Explicitly deferred from AK.2

- `peer_resolution.rs` and `runtime_health/peer_authority.rs`: AK.3 owns their
  separate alias/resolver cleanup. AK.6 independently preserves the
  TLS interop subset from the pre-AK.2 baseline; AK.2 deletes all active custom
  TLS code instead of retaining it in the daemon.
- Alias index, `PeerEndpoint`, and canonical IP-alias substitution: AK.3.
- Any native peer HTTP call: AK.4.
- Any endpoint state, timer, aggregate, or retry: AK.5.
- No replacement worker, task handle, per-message thread, broad peer scan, or
  immediate SQLite reload may enter AK.2.

## Required validation

- Pre-delete discovery: the marker-only commit applies the listed AK.2
  deprecations, then `cargo check --workspace` reports only the attributed
  AK.2 warning set. Do not add an `allow(deprecated)` to hide callers.
- Source gate rejects every deleted coordinator, PeerSync, policy, projection,
  and peer post-commit identifier named above from production code.
- Unit: host-qualified admission persists its origin ULID and immutable record
  once and starts no peer queue, peer thread, reload, socket, DNS, or nudge.
- Unit: local write retains its ordinary local nudge.
- Regression: a received peer write still takes the same `signal_local_post_write`
  nudge path as a local received HTTP write; no inbound local-host/same-IP
  branch exists.
- Migration: an existing SQLite database containing `peer_sync_policies`
  opens successfully and the obsolete table/index are absent afterward.
- OpenAPI baseline regeneration and `git diff --check` pass.
- Post-delete: no AK.2 deprecation marker or warning remains; `just lint` and
  `just test` pass.
- Smoke: run `just smoke localhost` and `just smoke local-ip` against an
  isolated test home/database; each host-qualified admission remains persisted
  and unnudgeted at the origin while the ordinary receiver path remains intact.

## Dependencies

Before every AK.2 development/fix round, merge AK.1 into AK.2. Start AK.2 as
soon as AK.1 is pushed; do not wait for QA. AK.2 must not merge to `develop`:
AK.4 restores delivery. Push AK.2, then start AK.3 with AK.2→AK.3 merge-forward.
AK.1 PR must merge before AK.2 PR completion.
`must_follow` is required because AK.2 applies AK.1's keep/discard decision;
it is not parallel-safe because both touch cross-host routing/provenance.
