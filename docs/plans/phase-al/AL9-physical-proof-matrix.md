# AL.9 physical-proof matrix

**Status:** partial local source-and-runtime proof only; this is not an
activation record and does not authorize Phase AM.

**Proof subject:** `9ceb7bee4676cc09cb9b4bfacd56e1fcf3da8612` on
`feature/pal-s9-physical-proof-ledger-freeze`, with AL.8 composition input
`9823712030d3d7d90629390f13f5daafa82c6888`. The subject details and the
separate activation preconditions are in [AL9-proof-subject.md](AL9-proof-subject.md).

## Common path under test

```text
CLI/graft write
  -> preferred_local_client
  -> HttpRuntimeClient::execute(ApiRequest<Write>)
  -> POST /v1/atm/messages (existing WriteRequest JSON)
  -> canonical_api_router
  -> ApiRouter dispatch
  -> StorageAndNudgeRouter / sealed storage boundary
  -> MessageReceivedHookEmitter, only after a new durable write
```

`preferred_local_client` selects an owner-authorized Unix socket on Unix when
the replacement configuration enables one, and the capability-authenticated
loopback record otherwise (including the explicit root-owned configuration).
It never silently changes adapter after a failed UDS request. The connector
alone changes physical setup; `HttpRuntimeClient` owns the one encoder,
`/v1/atm/messages` route, response decoder, deadline, and outcome mapping.

## Evidence matrix

| Adapter / assertion | Local evidence at `9ceb7bee` | Result | Remaining physical gate |
| --- | --- | --- | --- |
| In-process canonical write, storage before hook | `storage_and_nudge_router::tests::axum_route_persists_before_emitting_one_received_hook`; `message_handler::tests::local_and_peer_use_identical_write_json_and_one_dispatch` | Pass when run locally | No listener is involved; it is supporting path evidence, not activation proof. |
| New-write-only hook semantics | `axum_route_idempotent_duplicate_skips_the_second_received_hook` and `axum_route_hook_failure_returns_durable_success_with_warning` | Pass when run locally | None beyond the full physical adapter rows. |
| Unix UDS write | `uds_runtime_reaches_the_canonical_storage_and_received_hook_path`; `unix_socket_uses_the_shared_client_router_and_owner_only_endpoint` | Pass when run locally on Unix | Authorized host activation still required. |
| Loopback TCP write | `loopback_shared_client_uses_the_active_record_and_canonical_handler`; `loopback_and_uds_return_identical_canonical_json` | Pass when run locally | Authorized host activation plus an actual Windows run. |
| CLI local write selection | `atm-architecture::al9_cli_and_graft_send_use_the_selected_runtime_client` requires `preferred_local_client` plus awaited `async_transport.execute(Write)` and rejects compatibility dispatch from the send segment | Static/source proof | A switched-host CLI smoke owned by team-lead. |
| Graft outbound write selection | The architecture test locks `GraftClient::connect` and `AtmGraftClient::send_message` to the shared client. The registered runtime command is `just smoke graft-hermes`: it first requires `atm doctor --json` to prove the selected matched pair ready, then runs the installed `atm-graft` Python session's real `send`/nudge/read/ack round trip and writes a self-contained smoke report. It never starts or terminates a daemon. | Runtime command registered; no current AL.9 report artifact | The release operator runs `/daemon-switch`, verifies doctor, then runs `just smoke graft-hermes` from the configured profile with distinct registered sender/recipient identities. Retain the generated `site/reports/smoke/.../graft-hermes/` artifact. |
| Direct failure creates no retry/replay | `client::tests::direct_connector_failure_performs_exactly_one_exchange` | Pass when run locally | One physical refused-connection run can supplement this, but must not manufacture replay state. |
| M5 direct cross-host write | `DirectPeerTcpConfig`, `direct_peer_tcp_client`, and `direct_peer_listener_uses_the_canonical_router_and_normalizes_provenance` | **Documented rerun required** | In a fresh M5 proof worktree, configure receiver `ATM_HTTP_DIRECT_PEER_BIND=<private-m5-ip>:<port>` and `ATM_HTTP_DIRECT_PEER_SOURCE_HOST=<sender-host>`, then sender `ATM_HTTP_DIRECT_PEER_PORT=<port>`. Curl a normal `WriteRequest` JSON to `POST /v1/atm/messages`; record source SHA/diff, HTTP response, persisted message, and exactly-one received-hook evidence. This is a temporary plaintext, unauthenticated evidence adapter only—not production activation. |
| Windows physical proof | No current-runtime artifact exists | **Pending** | A real Windows runner must exercise the loopback row and benchmark; historical Phase AI output is baseline-only. |

## Explicit negative claims

- This matrix does **not** claim an ambient daemon has been switched, stopped,
  or inspected. The local graft smoke safety refusal is retained rather than
  bypassed.
- This matrix does **not** claim TLS, a peer DTO, a message array, resend, or
  replay behavior. The optional direct-peer listener is deliberately narrow:
  it uses the same `WriteRequest` route and canonical router as local ingress,
  and exists only for the documented M5 evidence rerun.
- The retained synchronous `atm_daemon_client` path is not used by CLI/graft
  **writes**. It remains only for synchronous read/ack/admin compatibility;
  [AL9-live-reference-graph.md](AL9-live-reference-graph.md) assigns its
  migration/deletion to AM.1.

## Reproducible local validation

```sh
cargo test -p atm-http-runtime --lib
cargo test -p atm-architecture al9_cli_and_graft_send_use_the_selected_runtime_client -- --nocapture
```

The individual test names, rather than a generic test count, are the durable
route-to-hook evidence. A passing command is necessary but insufficient for
the unexecuted Windows, graft, benchmark, M5, and activation rows. The M5 row
requires the documented fresh-worktree rerun; it does not claim that the run
has already occurred.
