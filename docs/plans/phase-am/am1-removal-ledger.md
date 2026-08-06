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
rg -n 'HttpFrameReader|write_http_request|read_http_response_with_frame_reader' crates
rg -n 'PEER_SOURCE_HOST_HEADER|PeerMessageArray|peer_sync_path_host' crates
rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|peer_delivery_observability' crates
rg -n 'rusqlite|atm_graft|atm-graft|tmux' crates/atm-daemon crates/atm-http-runtime
```

The TLS quarantine (`crates/atm-peer-tls-interop` and
`crates/atm-storage/src/tls.rs`) is excluded: it is neither production traffic
nor an AM deletion target without a later explicit decision.

## Production removal ledger

| ID | Legacy module / path | Remaining live callers / incoming edges | AL replacement | Owner | Planned validation | Deletion order |
| --- | --- | --- | --- | --- | --- | --- |
| AM1-RM-001 | `crates/atm-core/src/api/http_frame_reader.rs`; export and handwritten helpers in `crates/atm-core/src/api.rs` (`HttpFrameReader`, `read_http_request`, request/response writers/readers) | `atm-daemon::{local_ipc_transport::{request_worker,shutdown},local_tcp_transport,https_transport}`, `atm-daemon-client::{lib,http_exchange}`, and their direct tests | `atm-http-runtime` typed client and `message_handler::handle_message_request` | AM.2 | `rg -n 'HttpFrameReader|write_http_request|read_http_response_with_frame_reader' crates`; local + M5 smoke | after all listed callers are deleted; before AM.3/AM.4 consumers |
| AM1-RM-002 | `crates/atm-daemon-client/src/http_exchange.rs` and `lib.rs::{try_connect,exchange_request,exchange_tcp_request,exchange_uds_request}` | CLI `atm`, `atm-graft`, and daemon bootstrap composition depend on this raw local client | AL shared `DaemonApiClient` implementation in `atm-http-runtime` | AM.3 | `cargo tree -i atm-daemon-client`; local UDS/loopback smoke | after AM1-RM-001 and migration of every caller |
| AM1-RM-003 | `crates/atm-daemon/src/local_ipc_transport.rs` and submodules `accept_loop`, `connection_workers`, `request_worker`, `shutdown` | `composition`, `local_ipc_connection`, daemon tests and socket-record fixtures | AL local physical adapter plus one typed handler | AM.3 | `rg -n 'local_ipc_transport|HttpFrameReader' crates`; supported-platform local smoke | after AM1-RM-001; before its fixtures/deps |
| AM1-RM-004 | `crates/atm-daemon/src/local_tcp_transport.rs` | `composition`; Unix fallback from `local_ipc_transport`; Windows loopback setup; local transport tests | AL local physical adapter plus one typed handler | AM.3 | `rg -n 'local_tcp_transport|LocalTcpLoopbackServer' crates`; Windows/loopback smoke | after AM1-RM-003's fallback edge is removed |
| AM1-RM-005 | `crates/atm-daemon/src/https_transport.rs` (`HttpsTransport`, listener accept/read/write path, `route_peer_http_request`) | `composition`, `runtime_health`, `peer_drain_coordinator`, HTTPS tests | AL TLS physical adapter around shared typed client/handler | AM.4 | `rg -n 'HttpsTransport|route_peer_http_request|HttpFrameReader' crates/atm-daemon`; M5 direct-send and TLS-negative smoke | after AM1-RM-001 and after AM.5 removes coordinator caller |
| AM1-RM-006 | peer-only grammar/provenance in `crates/atm-core/src/api.rs` (`PEER_SOURCE_HOST_HEADER`, `peer_sync_path_host`, peer-sync route) | `https_transport` plaintext smoke/provenance normalization and raw API tests | authenticated TLS provenance supplied to the one AL handler, not an application header/body protocol | AM.4 | `rg -n 'PEER_SOURCE_HOST_HEADER|peer_sync_path_host|PeerMessageArray' crates`; route/body snapshot | after AM1-RM-005's peer path is gone |
| AM1-RM-007 | `crates/atm-daemon/src/runtime_health/peer_delivery_router.rs` and `post_commit_work.rs::PostCommitWorkKey::PeerDelivery` | `runtime_health::dispatch`, receiver persistence/post-commit signaling | direct persistence + AL receive hook; no sender-side delivery router | AM.5 | `rg -n 'peer_delivery_router|PostCommitWorkKey::PeerDelivery' crates`; failed-send integration accounting | after AM.4 removes peer ingress/egress |
| AM1-RM-008 | `crates/atm-daemon/src/peer_drain_coordinator.rs` (`PeerDrainCoordinator`, workers, peer jobs, sync deadline) | `runtime_health`, `composition::{start,stop}_peer_drain_coordinator`, `post_commit_work` | ordinary direct typed send outcome; no replay/recovery worker | AM.5 | `rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|peer_drain' crates`; no-background-work failure test | after AM1-RM-005 and AM1-RM-007 |
| AM1-RM-009 | `crates/atm-daemon/src/peer_delivery_observability.rs` (`PeerDeliveryProjection`, peer delivery events/status capacity) | `runtime_health`, `runtime_health::dispatch`, `peer_delivery_router`, coordinator, `tests/runtime_root/peer_observability.rs` | remove with replay; retained runtime request registry is not a replacement or deletion target | AM.5 | `rg -n 'peer_delivery_observability|PeerDeliveryProjection|PeerDeliveryEvent' crates`; doctor/status test removal review | after AM1-RM-007 and AM1-RM-008 |
| AM1-RM-010 | peer delivery doctor/status types `atm-core::doctor::{PeerDrainState,PeerLinkStatus,PeerLinkQuality}` and the daemon doctor projection consumers | `peer_delivery_observability`; doctor reporting/tests | remove only if the frozen graph confirms no non-replay consumer; otherwise retain the live non-peer doctor contract | AM.5 (conditional) | `rg -n 'PeerDrainState|PeerLinkStatus|PeerLinkQuality' crates docs`; API/doctor contract review | after AM1-RM-009; conditional on frozen graph |
| AM1-RM-011 | peer delivery/recovery test-only surfaces: `crates/atm-daemon/src/tests/runtime_root/{peer_observability,peer_reconciliation,peer_failure}.rs`, blocking-peer fixtures, and `scripts/smoke/{run_peer_pair,test_run_peer_pair,combine_inbound_peer_smoke,test_combine_inbound_peer_smoke}.py` | AM1-RM-005/007/008/009 implementation tests and peer-pair harness | retain only AL direct-send + received-hook tests; delete tests proving removed worker/protocol behavior | AM.4/AM.5 by row dependency | focused smoke + repository fixture search | after the implementation row each test proves |
| AM1-RM-012 | Cargo edges: `atm-daemon -> atm-daemon-client`; `atm` / `atm-graft -> atm-daemon-client`; any raw framing-only dependency identified after AM.2 | AM1-RM-002 callers and raw client path | `atm-http-runtime` client boundary | AM.3 | `cargo tree -i atm-daemon-client` and workspace build | after AM1-RM-002 and every listed caller is migrated |

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

1. AM.3 deletes raw-client callers, local worker callers, fixtures, then
   `atm-daemon-client` and local transport modules.
2. AM.4 deletes peer-route callers/fixtures, then `https_transport` and the
   peer-only core grammar/provenance symbols.
3. AM.2 deletes the shared raw API/frame-reader only after steps 1 and 2 have
   removed every compiled caller. Its sprint number is not authority; this
   caller-before-callee placement is.
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
AM.2--AM.5 must enable the category only in the same deletion PR that makes
the category empty, and must retain a mutation test in that PR. This is a
draft enforcement mechanism, not a premature compatibility tombstone.
