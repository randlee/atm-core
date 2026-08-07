# AL.9 adapter cutover table

**Status:** pre-activation plan. Team-lead must name the release operator and
record a switched-host observation before any row becomes active evidence.

| Adapter | Add / serving implementation | Activation trigger and owner | One active listener / publisher invariant | Retire action | Rollback owner and action |
| --- | --- | --- | --- | --- | --- |
| In-process router | `canonical_api_router` inside `atm-http-runtime` test/runtime assembly | Test harness only; no release activation | No socket and no endpoint record exist | Test process exits | Test harness drops the runtime |
| Unix UDS (Unix, non-root) | `HttpRuntime<Configured>::start` binds the configured owner-only `HOST_RUNTIME_SOCKET_FILE` | Team-lead-authorized release operator starts replacement daemon | The one `HttpRuntime` owns and removes its own socket; no endpoint-record publisher applies | Stop legacy listener before activating this listener; Phase AM deletes legacy code only after AL.9 accepts | Release operator stops replacement; removes only its owned socket through normal drain; keep legacy unchanged until a new approved proof |
| Loopback TCP | `HttpRuntime` binds loopback and `publish_loopback_endpoint_record` writes `local-http.json` | Same authorized release operator | One `HttpRuntime` listener publishes exactly one capability record after all enabled listeners bind; `LoopbackEndpointRecordGuard` removes only its own generation | Stop legacy listener/publisher before activation; Phase AM later deletes legacy publisher | Release operator drains replacement; its generation guard cleans its record without removing a successor record |
| CLI local write | `preferred_local_client` -> UDS or loopback `HttpRuntimeClient` | No listener; client becomes usable only after the corresponding runtime listener is active | Does not publish a record or bind a listener | AM.1 migrates retained synchronous read/ack/admin before deleting compatibility client | Release operator restores the previously approved client/runtime pair as one switch, not a client-only fallback |
| Graft local write | Same `preferred_local_client` / `HttpRuntimeClient` path as CLI | No listener; independent graft process connects only after runtime is active | Does not publish a record or bind a listener | AM.1 owns async non-write conversion and legacy client deletion | Same paired client/runtime rollback; do not attach to or kill an ambient daemon for smoke |
| Cross-host/M5 plain TCP | `DirectPeerTcpConfig` binds one explicit address and applies its configured source host before `canonical_api_router`; host-qualified CLI/graft writes use `direct_peer_tcp_client` | Team-lead-authorized release operator starts the receiver with `ATM_HTTP_DIRECT_PEER_BIND` and `ATM_HTTP_DIRECT_PEER_SOURCE_HOST`; sender supplies `ATM_HTTP_DIRECT_PEER_PORT` | The peer adapter owns no loopback endpoint record. One receiver listener is bound only after config preflight, and no client falls back after a peer failure | Disable the peer configuration and drain the replacement runtime; do not activate TLS or legacy peer machinery | Release operator parks the proof and restores the previously approved client/runtime pair; do not attach to or kill an ambient daemon |

## Hard activation conditions

Before a release operator switches any host, this table must be amended with:

1. operator identity, exact source SHA, binary version/path, and start time;
2. the observed process/listener for each enabled adapter and the loopback
   record publisher/generation;
3. a successful same-host CLI write, graft write, and direct-failure check;
4. the rollback command, named owner, and post-rollback listener/record check.

If any item fails, the release operator keeps the existing activation state,
parks AL, and does not authorize AM or a ledger freeze. This document makes no
claim that the ambient daemon currently running on this machine is either the
replacement or the legacy process.
