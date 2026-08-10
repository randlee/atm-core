# AM.1 Legacy Transport Removal Ledger

Status: refreshed draft inventory only (2026-08-10).  This is **not** a
deletion, guard-activation, or runtime-change authorization.  It was refreshed
on `feature/pam-s1-removal-ledger` after merging `origin/integrate/phase-am`
and `origin/integrate/phase-al`; the latter was `d8eac064` when reviewed.

AM.1 may freeze an inventory only after AL.9's accepted live-reference graph
**and** accepted physical/benchmark evidence.  The checked-in AL.9 provenance
record pins the graph at `9ceb7bee` but explicitly says it is static,
`host activation: not-yet-activated`, and still lists physical evidence rows
as required.  No explicit AL.9 acceptance record was found in the reviewed AL
documents, so this refresh records the acceptance state as **not proven** and
leaves freeze pending.  Later AL.13 cross-host smoke artifacts and the
AL.17/AL.19 optional Hermes/graft lane do not retroactively supply that missing
AL.9 acceptance.  The Tokio/Axum runtime remains the active proof subject; the
legacy `atm-daemon` is an AM removal subject and must not be started as a
shortcut for evidence.

The authoritative AM.1 sprint document deliberately has no frontmatter.  The
dispatch's generic frontmatter-completion criterion therefore does not apply;
this document remains a draft until the named AM owner accepts a freeze.

## Refresh task list

- [x] Merge the current AM and AL integration histories into this dedicated
  branch before inventorying it.
- [x] Re-read the AM plan, AM.1 sprint, AL/AM boundary checklist and transition
  document; retain the no-deletion/no-early-guard boundary.
- [x] Recheck current source, Cargo callers, test fixtures, architecture gates,
  AL.9/AL.13 evidence contracts, AL.17/AL.19 graft boundaries, and AL.11
  shutdown disposition.
- [x] Replace stale file and call-edge claims below with current paths.
- [x] Execute the draft guard's mutation suite and a currently-empty category;
  keep the guard unregistered while retained categories are non-empty.
- [ ] AM owner: obtain an explicit accepted AL.9 reference-graph plus
  physical/benchmark evidence record; then record its SHA/link before freezing
  this ledger.  Current checked-in evidence does not prove that acceptance.

## Repeatable inventory commands

Run from the repository root before each correction or deletion PR:

```sh
rg -n 'HttpFrameReader|read_http_request|decode_request|write_http_request|read_http_response_with_frame_reader' crates
rg -n 'PEER_SOURCE_HOST_HEADER|PeerMessageArray|peer_sync_path_host|route_peer_http_request' crates
rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|PeerDeliveryProjection|peer_delivery_observability|peer_delivery_router' crates
rg -n 'atm_daemon_client|atm-daemon-client|try_connect|exchange_request' crates --glob 'Cargo.toml' --glob '*.rs'
rg -n '^\\s*(use|extern crate)\\s+rusqlite|^\\s*rusqlite\\s*=' crates/atm-daemon crates/atm-http-runtime
rg --files crates | rg '(peer|tls|https)'
cargo tree -i atm-daemon-client
```

The `atm-peer-tls-interop` and `atm-storage/src/tls.rs` paths now exist and are
quarantined/reference-only AL artifacts.  They are not absent paths and are not
AM.1 deletion targets without a future, accepted reference graph proving an
incoming legacy edge.

## Current production removal ledger

| ID | Current legacy surface and incoming edges | Disposition / owner | Validation and caller-before-callee order |
| --- | --- | --- | --- |
| AM1-RM-001 | `atm-core/src/api/http_frame_reader.rs` and raw helpers exported by `api.rs`: `HttpFrameReader`, request/response readers/writers and `decode_request`.  Live callers are `atm-daemon-client`, legacy daemon local IPC/TCP transports, and API/unit tests. | Remove only after all raw callers are migrated; AM.2 owns callee removal.  `atm-http-runtime::{client,http1_server,message_handler}` is the typed replacement boundary, not evidence that legacy callers are already gone. | Search first command; local and cross-host smoke after migrations.  Follows RM-002 and RM-003. |
| AM1-RM-002 | `atm-daemon-client` synchronous local client: `http_exchange`, `try_connect`, `exchange_request`, compatibility/bootstrap support.  Live crate callers include `atm`, `atm-graft`, and legacy daemon tests/support. | AM.3 migrates/deletes remaining synchronous compatibility/read/admin callers only after the frozen graph separates them from the AL async write client. | `cargo tree -i atm-daemon-client`, caller search, UDS/loopback smoke.  Delete before RM-001 and remove Cargo edges last. |
| AM1-RM-003 | Legacy `atm-daemon` local listener code: `local_ipc_transport` submodules, `local_tcp_transport`, `local_ipc_connection`, and transport-specific request/connection workers.  They still use `HttpFrameReader`. | AM.3 removes this legacy listener family and its fixtures after a typed runtime replacement is the sole selected daemon.  Generic active-connection/shutdown accounting is a retain candidate. | Search paths and raw symbols; supported-platform local smoke.  Remove callers before RM-001. |
| AM1-RM-004 | Legacy peer-header compatibility marker `PEER_SOURCE_HOST_HEADER` in `atm-core/api.rs`; `atm-http-runtime/message_handler.rs` imports it solely to reject the header defensively.  No live `PeerMessageArray`, peer-sync route, or `route_peer_http_request` source was found. | Conditional AM.4: preserve a private/runtime defensive rejection or its replacement test before deleting the public legacy marker.  Do not reintroduce application peer provenance. | Header/route search and negative HTTP snapshot.  Current handler rejection is a required predecessor, not a removable peer sender. |
| AM1-RM-005 | Historical peer delivery coordinator, HTTPS transport, delivery projection, and peer observability files are already absent (`https_transport.rs`, `peer_drain_coordinator.rs`, `peer_delivery_observability.rs`).  `runtime_health/peer_delivery_router.rs` remains as a composition/architecture anchor; `post_commit_work.rs` contains only synchronous received-hook routing and an explicit no-background-work rule. | Record as **already removed / retain only the no-replay anchor**, not as an AM.5 future deletion.  Any future deletion must demonstrate it has no direct receive-hook responsibility. | Search peer/replay symbols and architecture gate.  Never delete the retained hook path merely because its historical name mentions peer delivery. |
| AM1-RM-006 | `peer_resolution.rs`, `runtime_health/peer_authority.rs`, trusted-peer storage, and `atm-peer-tls-interop`/storage TLS types remain physical-address/trust candidates.  They are not sender replay workers. | Conditional retain; an owner must identify an actual live AL physical-adapter edge before any removal.  No TLS activation is authorized by AM.1. | Peer/TLS path search and M5 direct-host smoke.  Do not infer deletion from old HTTPS names. |
| AM1-RM-007 | Legacy tmux emitter in `atm-daemon/src/message_received_emitter.rs` is live; the Tokio replacement selector is also live in `atm-daemon-bootstrap`.  `atm-graft` is a separate supported boundary, not a daemon dependency to erase blindly. | Guard only prohibited daemon graft edges and any future legacy tmux adapter selected for deletion.  Exclude the current accepted legacy emitter until its own owner has a replacement and migration proof. | Guard mutation plus harness-specific tests; preserve current received-hook behavior. |
| AM1-RM-008 | Direct SQLite is absent from daemon and HTTP-runtime manifests/source.  Storage remains behind `atm-storage` / `atm-storage-rusqlite` boundaries; architecture enforcement forbids the bad edges. | Already clean; retain as an active negative category once a deletion PR enables it. | Direct-SQLite guard success and architecture boundary tests. |
| AM1-RM-009 | Raw transport tests/fixtures remain in API, daemon local IPC/TCP, daemon-client, architecture enforcement, and smoke support.  The AL11 subprocess test gap was waived because it would start the frozen legacy binary; the AL11 `process::exit` UDS-leak code defect is fixed. | Delete tests with the implementation row they specify; retain AL13/AL9 typed smoke and the AL11 lifecycle decision record. | Fixture search, focused replacement tests, then full test/lint. |

## Topology and retained boundaries

```text
atm / atm-graft -> atm-daemon-client -> raw api/frame reader
legacy daemon local IPC/TCP -> raw api/frame reader
canonical HTTP handler -> defensive legacy-header rejection
durable write -> synchronous received-hook route -> supported tmux/graft selector
peer authority/DNS -> configured physical peer candidate (conditional retain)
```

The prior coordinator-to-HTTPS-to-replay chain is not a live topology row: its
implementation files are absent.  `peer_delivery_router` and
`post_commit_work` must instead be reviewed by their actual synchronous
received-hook behavior.  This avoids deleting current AL behavior based on
historical filenames.

`DaemonApiClient` is sealed under ADR-001.  AM.1 introduces no trait,
implementation, or crate-boundary change; any later client migration must
review the existing sealed implementations rather than adding an unauthorized
implementation.  The inventory also found no AM.1-owned newtype, lock, or
error surface requiring a Rust-pattern remediation (RBP-001/003/004/006).

## Draft negative guard and mutation proof

`scripts/phase-am/check_legacy_transport_removal.py` covers raw framing,
peer-only ingress, resend/replay, direct SQLite, and daemon harness edges.
Its tests are `.just/tests/test_phase_am_legacy_transport_guard.py`.

2026-08-10 evidence:

```text
python3 .just/tests/test_phase_am_legacy_transport_guard.py -v  # 10 passed
python3 scripts/phase-am/check_legacy_transport_removal.py --category direct-sqlite  # passed
```

The mutation tests prove each category fails for a reintroduced representative
symbol; the direct-SQLite category is currently empty.  Other categories are
intentionally non-empty because RM-001--RM-004 and RM-007 remain live.  The
guard must stay out of `just lint` and out of integration until the owning
deletion PR makes its selected category empty, enables that category in the
same PR, and retains its mutation test.

## Freeze and deletion rules

1. A future AM owner replaces the pending freeze task only with a concrete,
   accepted AL.9 live-reference graph SHA **and** its physical/benchmark
   evidence links.  A static graph or later-sprint artifact is insufficient.
2. No row is removed based solely on a phase number, an historical document, or
   an absent predecessor file.  Re-run the inventory, identify compiled callers,
   then delete caller before callee.
3. Delete implementation, Cargo edge, fixtures, docs, and selected negative
   guard in one owned PR; run focused tests, `just lint`, and `just test`.
4. Do not start, patch, or use frozen `atm-daemon` to prove AL runtime behavior.
   AL11's waived binary-level regression gap is tracked as a deletion-era
   decision, not authority to revive the legacy runtime.
