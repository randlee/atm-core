# AM.6 Closure Proof

This report closes Phase AM against the frozen AM.1 ledger rows, rather than
making a repository-wide claim that every historical transport term is absent.
The evidence is source-level and is intentionally limited to AM's deletion
scope; the retained public HTTP contract is not redesigned here.

## Ledger-row evidence

| Row | Final source/Cargo/fixture/doc evidence | Owner and final guard or proof |
| --- | --- | --- |
| AM1-RM-001 | `HttpFrameReader`, handwritten request/response helpers, and `decode_request` have no production definition; `scripts/phase-am/check_legacy_transport_removal.py` rejects all of them. | AM.2; `raw-framing` is enabled in `just lint` and has representative mutations for `HttpFrameReader` and `decode_request`. |
| AM1-RM-002 | `atm-daemon-client` remains only for retained non-write compatibility and availability calls; its raw framing was replaced with the typed runtime client. | AM.2; `cargo tree -i atm-daemon-client` and the no-write-path architecture tests retain this narrow compatibility boundary. |
| AM1-RM-003 | The legacy daemon local IPC/TCP worker modules and fixtures are absent; `atm-daemon` starts the replacement bootstrap/runtime listener pair. | AM.3; boundary enforcement, local UDS/loopback smoke, and the raw-framing guard prove the deletion. |
| AM1-RM-004 | `PEER_SOURCE_HOST_HEADER`, peer-array grammar, peer-sync routes, and peer-only request DTOs are absent from production. The HTTP boundary only rejects the retired header. | AM.4; `peer-ingress` is enabled in `just lint` with a representative mutation and malformed-header coverage. |
| AM1-RM-005 | Query/cursor/replay state and serialized `peerOutbound.request` are absent. `peerOutbound.host` is direct routing metadata; `post_commit_work` has no background work. | AM.5; `resend-replay` is enabled in `just lint` with scheduler/query/serialized-payload mutations and direct-host accounting coverage. |
| AM1-RM-006 | `atm-peer-tls-interop` and storage TLS types are reference-only physical-adapter material, with no AM deletion authorization or production-routing claim. | Conditional retain; M5 direct-host smoke is the physical-path evidence. |
| AM1-RM-007 | The supported tmux received-hook emitter is retained; the guard prohibits daemon graft edges while excluding that still-live emitter. | Conditional retain; harness tests and received-hook tests prove the current path without calling it replay. |
| AM1-RM-008 | `atm-daemon` and `atm-http-runtime` have neither a `rusqlite` manifest dependency nor a direct import. | Active `direct-sqlite` guard is enabled in `just lint`; source and Cargo mutation tests cover both rules. |
| AM1-RM-009 | Deleted raw-worker fixtures left with their owners; retained typed AL smoke and lifecycle records remain. `scripts/smoke/analyze_logs.py` is retained generic structured-log analysis, not a replay/transport implementation. | AM.2--AM.5; full test/lint and the AL proof suite cover the retained fixtures. |

## Enabled-guard mutation contract

The enabled categories are `raw-framing`, `peer-ingress`, `resend-replay`, and
`direct-sqlite`. `.just/tests/test_phase_am_legacy_transport_guard.py` mutates
each enabled rule: both raw-framing rules, the peer-ingress rule, the
resend/replay rule, and both direct-SQLite rules. The `daemon-harness`
category remains deliberately disabled because its accepted tmux emitter is
live; its mutation tests are retained but it is not a closure claim.

## Composition-only audit

`atm-daemon` remains the lifecycle/composition root: ownership, readiness,
shutdown, configuration/health projection, and the bounded received-hook
adapter live there. It has no raw socket/framing implementation and no direct
SQLite dependency. `atm-http-runtime` owns the maintained Axum server,
authenticated connector selection, request codec use, and typed client.
`atm-core` owns the contracts (`WriteRequest`, `ApiRequest`, `ApiResponse`,
`ApiRouter`, `DaemonApiClient`) and storage traits.

The remaining module inventory was reviewed as follows:

| Crate | Modules and disposition |
| --- | --- |
| `atm-daemon` | `active_connection_registry`, observability, lifecycle/ownership/readiness, local admission, shutdown, and worker support are lifecycle composition. `daemon_worker_join` is retained only as the bounded join helper consumed by `active_connection_registry`. `non_claude_outbound_runtime` had no non-test caller and is deleted with its retired manifest. The unselected synchronous `DaemonRequestDispatcher` stack (`runtime_health`, `dispatch`, `peer_delivery_router`, status cache, and its test-only consumers) is deleted. `message_received_emitter` is the explicitly retained harness adapter. `peer_resolution` is physical-address resolution, not peer ingress grammar. |
| `atm-http-runtime` | `client`, `message_handler`, `storage_and_nudge_router`, `http1_server`, `unix_socket`, `loopback_tcp`, `private_staging`, and `runtime_health` form the sole maintained typed HTTP client/server implementation and its listener lifecycle. |

## QA handoff: singular production write path

The following is the one production write path QA should trace. Test doubles
are deliberately excluded.

1. Public type/schema oracle: `atm-core/src/send/mod.rs` `WriteRequest`, with
   route schema/codec constants in `atm-core/src/api.rs`.
2. Client implementation: `atm-http-runtime/src/client.rs`, selected typed
   HTTP client (`DirectPeerWriteClient` for a host-qualified direct write).
3. Router: `atm-http-runtime/src/message_handler.rs` `CanonicalWriteHandler`
   and `canonical_message_router`.
4. Runtime assembly: `atm-daemon-bootstrap/src/lib.rs`
   constructs `StorageAndNudgeRouter` and passes it to `HttpRuntimeBuilder`.
5. Dispatch and durable write: `atm-http-runtime/src/storage_and_nudge_router.rs`
   `StorageAndNudgeRouter::write` calls `commit_write`, whose async storage
   admission finishes the durable write.
6. Received-hook call site: that same router's `emit_received_hook`, called only
   after a newly persisted write. It is advisory after durability and does not
   schedule recovery work.

## Dynamic evidence

The following commands exercised the matched, signed M5 release pair at
`6a7e0e0399ef782176e0d20d7d593942154e0598` (`1.4.1-beta-ai-1`):

| Proof | Result | Public artifact |
| --- | --- | --- |
| Localhost smoke | PASS; 10 iterations of send, required acknowledgement, content, and acknowledgement-reply checks. | `site/reports/smoke/macos/rand-m5.local/20260811T000951322910Z-pid81552-localhost/` |
| Same-host IP smoke | PASS; 10 iterations using the advertised M5 address. | `site/reports/smoke/macos/rand-m5.local/20260811T001002232093Z-pid82203-local-ip/` |
| M5-to-M4 and M4-to-M5 smoke | PASS; 120 cases across 10 iterations, including both direct directions and all acknowledgement/content checks. M4 used its registered `arch-ctm` identity and explicit `/opt/homebrew/bin/atm` path. | `site/reports/smoke/macos/rand-m5.local/20260811T001934627915Z-pid86365-crosshost-send/` |
| Active-hook UDS admission | PASS; 354,000/354,000 writes accepted in 20.017 s, 17,779.88 median admissions/s, and the exact 354,000-row durable count passed after restart. | `site/reports/send-message-benchmark/20260811-001638.942602-m5-arm64-01-uds-f1.json` |

The supplied `20260801-072313.590684-mac-arm64-01-uds-f1.json` AL.9 artifact
is itself marked failed, so it is not a valid comparison baseline; the AL.9
gate now retracts that false baseline claim. No genuinely passing pre-AL
UDS/f1 artifact is retained. Consequently the M5 result is a standalone
capacity-and-durability proof, not a regression comparison: it proves the
required 1,000 admissions/s floor, error-free public admission, and exact
post-restart durability on physical M5 hardware, but cannot prove a percentage
change against an unavailable like-for-like baseline. The runner preserves a
completed profile and schema-valid elapsed duration when it rejects such a
baseline, rather than masking that diagnosis with a report serialization
failure. The successful final run retains its full raw trace locally.
