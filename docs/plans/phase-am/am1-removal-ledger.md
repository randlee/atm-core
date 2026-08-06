# AM.1 Legacy Transport Removal Ledger

Status: draft inventory only. This document neither deletes production code nor
activates a guard. It is based on the live-reference graph after AL.1/AL.2
were merged into `feature/pam-s1-removal-ledger`; AM.1 freezes it only after
AL.9 proves the new runtime live. The lifecycle is deliberately:

`AM.1 draft -> AL.9 accepted live-reference graph -> AM.1 freeze -> AM.2--AM.5 consume`.

Before each AM.1 correction or freeze pass, merge `integrate/phase-al` and
re-run the inventory commands below. A row is not removable merely because a
later phase intends to replace it.

## Inventory method

The draft was produced with these repeatable commands (run from repository
root):

```sh
rg -n 'HttpFrameReader|read_http_request|write_http_request|read_http_response_with_frame_reader' crates
rg -n 'PEER_SOURCE_HOST_HEADER|PeerMessageArray|peer_sync_path_host' crates
rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|peer_delivery_observability' crates
rg -n 'rusqlite|atm_graft|atm-graft|tmux' crates/atm-daemon crates/atm-http-runtime
```

TLS-quarantine review: the historical `crates/atm-peer-tls-interop` and
`crates/atm-storage/src/tls.rs` paths are absent in this checkout, so no
nonexistent path is excluded from AM. Current TLS physical-adapter candidates
are recorded in AM1-RM-014; they are retained unless the frozen AL.9 graph
proves a specific edge obsolete.

## FIX-AM1-1 finding task list

- [x] AM1-FINAL-001: align caller-before-callee prerequisites for raw framing.
- [x] AM1-FINAL-002: inventory the compiled `atm::composition` raw-frame test
  caller and make the validation search find it.
- [x] AM1-FINAL-003: record the canonical handler's peer-header dependency and
  its required defensive-check migration.
- [x] AM1-FINAL-004: exclude the accepted AL tmux receiver from the legacy
  daemon-harness guard, with a regression test.
- [x] AM1-FINAL-005: replace stale TLS quarantine paths with current evidence.
- [x] AM1-FINAL-006: exclude the hyphenated runtime test-support crate, with a
  regression test.
- [x] AM1-FINAL-007: include `peer_delivery_observability` in the replay guard,
  with a mutation test.

## AM.1 critical-review task list

This list records the post-implementation review rather than silently treating
the first draft as complete. Items are closed only with the evidence named.

- [x] Re-merge `origin/integrate/phase-al` before the review/fix pass; it was
  already up to date.
- [x] Expand the ledger beyond top-level modules to name local submodules,
  raw-client callers, peer authority/resolution retainers, test fixtures, and
  concrete Cargo edges.
- [x] Record every identified observability/capacity/doctor/dashboard/config
  surface as remove, retain, conditional, or proven absent.
- [x] Make draft guard activation category-selective so an AM.2--AM.5 PR can
  enable only the category it actually deleted.
- [x] Make guard scanning code-aware enough to ignore comments and prove its
  command-line nonzero exit on a representative mutation.
- [x] Re-run the full test gate and ensure the guard remains unregistered from
  `just lint` while its forbidden production symbols remain live.

## Production removal ledger

| ID | Legacy module / path | Remaining live callers / incoming edges | AL replacement | Owner | Planned validation | Deletion order |
| --- | --- | --- | --- | --- | --- | --- |
| AM1-RM-001 | `crates/atm-core/src/api/http_frame_reader.rs`; export and handwritten helpers in `crates/atm-core/src/api.rs` (`HttpFrameReader`, `read_http_request`, request/response writers/readers) | `atm-daemon::{local_ipc_transport::{request_worker,shutdown},local_tcp_transport,https_transport}`, `atm-daemon-client::{lib,http_exchange}`, `atm::composition`'s compiled `#[cfg(test)]` parity test (`decode_request`, `read_http_request`, `write_http_request`), and their direct tests | `atm-http-runtime` typed client and `message_handler::handle_message_request` | AM.2 | `rg -n 'HttpFrameReader|read_http_request|write_http_request|read_http_response_with_frame_reader' crates`; local + M5 smoke | delete only after every listed caller is deleted **or migrated off these symbols**; it is the callee and therefore follows AM1-RM-002/003/005's raw-frame edges |
| AM1-RM-002 | `crates/atm-daemon-client/src/http_exchange.rs` and `lib.rs::{try_connect,exchange_request,exchange_tcp_request,exchange_uds_request}` | CLI `atm`, `atm-graft`, and daemon bootstrap composition depend on this raw local client | AL shared `DaemonApiClient` implementation in `atm-http-runtime` | AM.3 | `cargo tree -i atm-daemon-client`; local UDS/loopback smoke | migrate/delete before AM1-RM-001; remove its Cargo edge only after every caller is migrated |
| AM1-RM-003 | `crates/atm-daemon/src/local_ipc_transport.rs` and submodules `accept_loop`, `connection_workers`, `request_worker`, `shutdown` | `composition`, `local_ipc_connection`, daemon tests and socket-record fixtures | AL local physical adapter plus one typed handler | AM.3 | `rg -n 'local_ipc_transport|HttpFrameReader' crates`; supported-platform local smoke | migrate/delete its raw-frame edge before AM1-RM-001; before its fixtures/deps |
| AM1-RM-004 | `crates/atm-daemon/src/local_tcp_transport.rs` | `composition`; Unix fallback from `local_ipc_transport`; Windows loopback setup; local transport tests | AL local physical adapter plus one typed handler | AM.3 | `rg -n 'local_tcp_transport|LocalTcpLoopbackServer' crates`; Windows/loopback smoke | after AM1-RM-003's fallback edge is removed |
| AM1-RM-005 | `crates/atm-daemon/src/https_transport.rs` (`HttpsTransport`, listener accept/read/write path, `route_peer_http_request`) | `composition`, `runtime_health`, `peer_drain_coordinator`, HTTPS tests | AL TLS physical adapter around shared typed client/handler | AM.4 | `rg -n 'HttpsTransport|route_peer_http_request|HttpFrameReader' crates/atm-daemon`; M5 direct-send and TLS-negative smoke | remove/migrate its raw-frame edge before AM1-RM-001; the coordinator→`HttpsTransport` edge must itself be removed or retargeted before this module is deleted |
| AM1-RM-006 | peer-only grammar/provenance in `crates/atm-core/src/api.rs` (`PEER_SOURCE_HOST_HEADER`, `peer_sync_path_host`, peer-sync route) | `https_transport` plaintext smoke/provenance normalization, raw API tests, and `atm-http-runtime/src/message_handler.rs` defensive rejection of the legacy header (including its tests) | authenticated TLS provenance supplied to the one AL handler, not an application header/body protocol | AM.4 | `rg -n 'PEER_SOURCE_HOST_HEADER|peer_sync_path_host|PeerMessageArray' crates`; route/body snapshot | before deleting the public core header symbol, AM.4 must move the canonical handler's rejection to a runtime-private legacy-header sentinel and preserve the rejection test; then delete the peer protocol callers |
| AM1-RM-007 | `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs` and `post_commit_work.rs::PostCommitWorkKey::PeerDelivery` | `runtime_health::dispatch`, receiver persistence/post-commit signaling | direct persistence + AL receive hook; no sender-side delivery router | AM.5 | `rg -n 'peer_delivery_router|PostCommitWorkKey::PeerDelivery' crates`; failed-send integration accounting | after AM.4 removes peer ingress/egress |
| AM1-RM-008 | `crates/atm-daemon/src/peer_drain_coordinator.rs` (`PeerDrainCoordinator`, workers, peer jobs, sync deadline) | `runtime_health`, `composition::{start,stop}_peer_drain_coordinator`, `post_commit_work` | ordinary direct typed send outcome; no replay/recovery worker | AM.5 | `rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|peer_drain' crates`; no-background-work failure test | after AM1-RM-005 and AM1-RM-007 |
| AM1-RM-009 | `crates/atm-daemon/src/peer_delivery_observability.rs` (`PeerDeliveryProjection`, peer delivery events/status capacity) | `runtime_health`, `runtime_health::dispatch`, `peer_delivery_router`, coordinator, `tests/runtime_root/peer_observability.rs` | remove with replay; retained runtime request registry is not a replacement or deletion target | AM.5 | `rg -n 'peer_delivery_observability|PeerDeliveryProjection|PeerDeliveryEvent' crates`; doctor/status test removal review | after AM1-RM-007 and AM1-RM-008 |
| AM1-RM-010 | peer delivery doctor/status types `atm-core::doctor::{PeerDrainState,PeerLinkStatus,PeerLinkQuality}` and the daemon doctor projection consumers | `peer_delivery_observability`; doctor reporting/tests | remove only if the frozen graph confirms no non-replay consumer; otherwise retain the live non-peer doctor contract | AM.5 (conditional) | `rg -n 'PeerDrainState|PeerLinkStatus|PeerLinkQuality' crates docs`; API/doctor contract review | after AM1-RM-009; conditional on frozen graph |
| AM1-RM-011 | peer delivery/recovery test-only surfaces: `crates/atm-daemon/src/tests/runtime_root/{peer_observability,peer_reconciliation,peer_failure}.rs`, blocking-peer fixtures, and `scripts/smoke/{run_peer_pair,test_run_peer_pair,combine_inbound_peer_smoke,test_combine_inbound_peer_smoke}.py` | AM1-RM-005/007/008/009 implementation tests and peer-pair harness | retain only AL direct-send + received-hook tests; delete tests proving removed worker/protocol behavior | AM.4/AM.5 by row dependency | focused smoke + repository fixture search | after the implementation row each test proves |
| AM1-RM-012 | Cargo edges: `atm-daemon -> atm-daemon-client`; `atm` / `atm-graft -> atm-daemon-client`; any raw framing-only dependency identified after AM.2 | AM1-RM-002 callers and raw client path | `atm-http-runtime` client boundary | AM.3 | `cargo tree -i atm-daemon-client` and workspace build | after AM1-RM-002 and every listed caller is migrated |
| AM1-RM-013 | `crates/atm-daemon/src/{request_worker,local_ipc_connection}.rs`, plus `local_ipc_transport/{accept_loop,connection_workers}.rs` | local IPC/TCP transport modules and daemon tests | retain only generic active-request shutdown accounting; delete transport-specific worker/connection helpers with AM.3 | AM.3 (split retain/delete at freeze) | `rg -n 'request_worker|local_ipc_connection|connection_workers|accept_loop' crates/atm-daemon` | local transport callers before helper callee |
| AM1-RM-014 | `crates/atm-daemon/src/{peer_resolution.rs,runtime_health/peer_authority.rs}` and peer configuration storage calls | `https_transport`, daemon composition, trusted-peer configuration | retain only AL TLS physical address/authentication resolution; delete any peer application-routing/replay edge with AM.4/AM.5 | AM.4/AM.5 (conditional) | `rg -n 'peer_resolution|peer_authority|resolve_peer_socket_addresses' crates`; M5 mTLS-negative smoke | after `https_transport` is replaced; retain only frozen physical-adapter edges |
| AM1-RM-015 | raw transport fixture/docs: local socket-record tests (`tests/local_ipc_depth.rs`), `atm-daemon-client` compatibility/local transport tests, API frame-reader tests, and raw transport references in `crates/atm-architecture/tests/boundary_enforcement.rs` | AM1-RM-001--004 | AL typed-client/handler parity and local smoke fixtures | AM.2/AM.3 by proven owner | `rg -l 'HttpFrameReader|atm-daemon-client|local_ipc_transport|local_tcp_transport' crates scripts docs` | delete each fixture only after its asserted legacy implementation is gone |

`active_connection_registry`, `local_ipc_connection`, and generic shutdown
drain accounting are **retain** candidates: they account for active request
lifecycles, not peer resend/replay. AM.5 must not remove them without frozen
graph evidence that they only serve an AM removal row.

## Observability, capacity, doctor, dashboard, and configuration dispositions

| Surface | Disposition | Rationale / owner |
| --- | --- | --- |
| `peer_delivery_observability::PeerDeliveryProjection` and `MAX_PEER_LINK_STATUS_ENTRIES` | Remove in AM.5 | It projects recovery/drain events and has no independent direct-send consumer. |
| `runtime_health::peer_delivery_router` / post-commit peer-delivery key | Remove in AM.5 | Sender-side handoff to replay coordinator. |
| `PeerDrainState`, `PeerLinkStatus`, `PeerLinkQuality`, doctor serialization | Conditional AM.5 removal | Delete with the projection only if AL.9 frozen graph shows no retained doctor consumer; otherwise retain and document a non-replay owner. |
| `active_connection_registry` capacity/drain metrics | Retain | Generic request/shutdown lifecycle accounting, explicitly outside peer replay scope. |
| `MAX_KEEP_ALIVE_REQUESTS` and local listener overload response | Retain | Active listener admission/capacity control, not peer delivery state. |
| peer recovery/deadline settings and doctor output | Remove in AM.5 | They only configure/report the coordinator; strict config upgrade must reject removed keys rather than silently ignore them. |
| `scripts/smoke/run_peer_pair.py` dashboard/event fixtures | Remove or rewrite in AM.4/AM.5 | Keep only if changed to prove AL's canonical direct route; do not preserve peer-delivery event assertions. |
| `scripts/smoke/analyze_logs.py` and `test_analyze_logs.py` `peer_delivery_confirmed` dashboard assertion | Remove or rewrite in AM.5 | It consumes the replay-era outcome event; replacement proof is direct-send result plus received hook. |
| `PEER_DELIVERY_WORKER_DEADLINE`, `PEER_SYNC_REQUEST_DEADLINE`, `PEER_DRAIN_JOIN_POLICY`, `PEER_JOB_JOIN_POLICY` | Remove in AM.5 | Concrete coordinator-only timing/worker state; no config-file key was found by the recorded inventory. |
| Config-file parser keys for peer recovery/retry | Proven absent | The current source inventory found no user-configurable peer recovery/retry key. AM.5 must preserve this result or record any key introduced before freeze. |
| `peer_resolution` and `runtime_health::peer_authority` | Conditional retain | They are physical TLS DNS/trust resolution candidates, not automatically replay state; frozen AL.9 graph determines the surviving facade owner. |

No dashboard/config key is presumed removable merely by name; AM.5 must cite
the frozen graph and delete its tests/docs/config parsing in the same PR.

## Call graph and required topological deletion order

The arrows below point from caller to callee; therefore callers are deleted
before the symbols they call.

```text
CLI/atm-graft -> atm-daemon-client -> atm-core raw API/frame reader
composition -> local_ipc_transport -> local_tcp_transport -> raw API/frame reader
composition/runtime_health -> https_transport -> raw API/frame reader
runtime_health::dispatch -> peer_delivery_router -> post_commit peer key
post_commit peer key -> PeerDrainCoordinator -> HttpsTransport
runtime_health/dispatch/coordinator -> PeerDeliveryProjection -> doctor peer status types
```

Frozen order (subject to AL.9 live-reference proof):

1. AM.3 migrates or deletes raw-client and local-worker edges, including the
   compiled `atm::composition` parity test, then removes their fixtures and
   `atm-daemon-client`/local modules where no caller remains.
2. AM.4 migrates the coordinator→`HttpsTransport` edge, deletes peer-route
   callers/fixtures and the public peer protocol, while preserving the
   canonical handler's defensive rejection through its private sentinel.
3. AM.2 deletes the shared raw API/frame-reader only after steps 1 and 2 have
   removed every compiled raw-frame caller. Its sprint number is not authority;
   this caller-before-callee placement is.
4. AM.5 deletes router/post-commit callers, coordinator workers, projection,
   conditional doctor types, and their config/dashboard/test/dependency rows.

If AL.9's graph shows an additional caller, it is inserted before its callee;
if a purported legacy symbol has no compiled caller, record its search proof
as dead rather than deleting it under an assumed owner.

## Draft guard and activation contract

[`scripts/phase-am/check_legacy_transport_removal.py`](../../../scripts/phase-am/check_legacy_transport_removal.py)
defines the future negative checks for raw framing, peer-only ingress,
resend/replay, direct SQLite in daemon/runtime, and daemon tmux/graft edges.
Its mutation tests are in
[`.just/tests/test_phase_am_legacy_transport_guard.py`](../../../.just/tests/test_phase_am_legacy_transport_guard.py)
and prove each category rejects a representative reintroduced symbol.

The guard is intentionally **not** registered in `just lint` yet because the
draft inventory demonstrates each category still has live production uses.
It is not to be merged into `integrate/phase-am` while live symbols remain.
AM.2--AM.5 must copy or merge the applicable draft and enable it with one or
more `--category` arguments only in the same deletion PR that makes those
categories empty, then retain the mutation test in that PR. This is a draft
enforcement mechanism, not a premature compatibility tombstone.
