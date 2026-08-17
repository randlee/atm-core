# AL.9 physical-proof matrix

**Status:** reconciled physical-proof record for the AL.9/AM transition. The
CLI, graft, and M5<->M4 cross-host runtime rows below have retained physical
evidence. Windows local proof is retained; the unavailable Windows<->M4 live
cross-host lane is an explicit Phase AP environment deferral, not an ATM
transport failure.

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
| CLI local write selection | `atm-architecture::al9_cli_and_graft_send_use_the_selected_runtime_client` requires `preferred_local_client` plus awaited `async_transport.execute(Write)` and rejects compatibility dispatch from the send segment. The later matched-pair M5<->M4 artifact records 120 CLI send/read/ack content cases in both directions, with distinct registered `rand-m5`/`arch-ctm` identities and M4's explicit `/opt/homebrew/bin/atm` path: [`am6-closure-proof.md`](../phase-am/am6-closure-proof.md#dynamic-evidence), `site/reports/smoke/macos/rand-m5.local/20260811T001934627915Z-pid86365-crosshost-send/`. | **Satisfied** — switched-host CLI proof retained. | No additional AL.9 CLI write gate. |
| Graft outbound write selection | The architecture test locks `GraftClient::connect` and `AtmGraftClient::send_message` to the shared client. The registered runtime command remains `just smoke graft-hermes`, but the literal command name was not used. Its required live Python-graft send/ack intent is nevertheless covered by the installed `hermes-atm` native-tools proof: SkillRX reported a post-daemon-cycle native `atm_send` retry success with durable message ID `01KZSSREKYM7G39237P0YQ3CW3` in ATM message [`01KZSSSG878QYFGBJVDWTJHXKG`](../../atm-dev/01KZSSSG878QYFGBJVDWTJHXKG), then reported the same native `atm_send` acknowledgement path's pending state cleared in [`01KZSVA533HB2XG0YN2X4NB97G`](../../atm-dev/01KZSVA533HB2XG0YN2X4NB97G). This is live installed `hermes-atm` over the shipped Python `atm_graft` session, not a hand-edited harness. | **Satisfied by equivalent live Python-graft evidence** — the prescribed smoke script itself was not run. | Retain the two ATM message IDs as the proof receipt; no claim is made that `just smoke graft-hermes` produced a report artifact. |
| Direct failure creates no retry/replay | `client::tests::direct_connector_failure_performs_exactly_one_exchange` | Pass when run locally | One physical refused-connection run can supplement this, but must not manufacture replay state. |
| M5 direct cross-host write | `DirectPeerTcpConfig`, `direct_peer_tcp_client`, and `direct_peer_listener_uses_the_canonical_router_and_normalizes_provenance`. The matched M5<->M4 release-pair smoke exercised both direct directions and acknowledgement/content checks for 120 cases: [`am6-closure-proof.md`](../phase-am/am6-closure-proof.md#dynamic-evidence), `site/reports/smoke/macos/rand-m5.local/20260811T001934627915Z-pid86365-crosshost-send/`. | **Satisfied** — real M5<->M4 direct cross-host write proof retained; no AL.7/TLS reuse is claimed. | No additional AL.9 direct-write gate. TLS remains separately out of scope. |
| Windows physical proof | Windows local daemon, test suite, and FastPC4 loopback-TCP benchmark proof pass; see [`release-highlights-phase-am.md`](../../release-highlights-phase-am.md#platform-and-cross-host-status). | **Explicitly deferred to Phase AP** — not a code or transport failure. | Windows<->M4 live cross-host proof awaits the documented VPN/DNS reachability constraint; Phase AP begins with that outbound-initiated proof. |

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

The individual test names, rather than a generic test count, are durable
route-to-hook evidence. The reconciled CLI, graft, and M5 rows above add their
specific physical receipts; the Windows live cross-host row remains an explicit
Phase AP environment deferral.
