# Phase AL/AM — Boundary Transition Inventory

Status: binding boundary-change inventory

The baseline manifest inventory is pinned by path and source SHA in
[`phase-al-am-baseline-boundary-manifests.md`](phase-al-am-baseline-boundary-manifests.md).
Its count is informative only and must not be used as a completion criterion.
This inventory reviews those records' relevance to AL/AM and makes one rule
explicit:

> A boundary manifest, boundary document section, allowlist, crate export, or
> lint rule is created, renamed, retired, or deleted **in the same sprint and
> PR as its implementing code change**. Planning may name the required
> artifact, but must not modify a boundary definition before the code exists.

The implementer must read ADR-001 before touching `sealed`, an allowlist, or a
cross-crate implementation. AL/AM must not widen `sealed` visibility or add an
unauthorized `sealed::Sealed` implementation.

## AL.1: boundary work that lands with the code

| Code introduced/transplanted in the AL.1 PR | Boundary artifacts changed in that same PR | Required rule |
|---|---|---|
| Accepted AK.11 receiver-only hook contract | Add `boundaries/atm-core/message-received-hook-emitter.toml`; delete `boundaries/atm-core/post-send-hook-emitter.toml`; update `docs/atm-core/boundaries.md`, `crates/atm-core/src/boundary`, and public re-export together. | The new sealed `MessageReceivedHookEmitter` is post-new-persistence only. The old name is compile-failing/deleted, never an active compatibility trait. |
| Graft receiver implementation, if the accepted AK.11 code includes it | Add the `atm-graft` receiver boundary manifest/document in the same PR; delete `graft-post-send-port.toml` and the daemon-facing graft-post-send contract in the same PR. | `atm_graft::nudge_sink::GraftReceiveHook` is receiver-owned. `atm-daemon` and `atm-http-runtime` gain no `atm-graft` dependency or implementation. |
| `crates/atm-http-runtime` construction facade | Add `boundaries/atm-http-runtime/http-runtime.toml` and its concise crate boundary document in the same PR. | Its facade may own maintained HTTP/TLS runtime mechanics only; allowed ATM dependency is `atm-core` contracts. It forbids Rusqlite/storage concrete types, tmux, graft, CLI/bootstrap, peer DTOs, replay, and business routing. |

No other new core trait is authorized. In particular, AL.1 does **not** add a
`PeerClient`, `PeerWrite`, `HttpMessage`, queue, scheduler, or runtime-private
storage trait. The existing sealed `DaemonApiClient`, `ApiRouter`, storage
traits, and `AtmError` contract are consumed unchanged.

## Existing boundaries: action by phase

| Existing record(s) reviewed | AL action | AM action | Closure rule |
|---|---|---|---|
| `mail-store`, `message-store`, `nudge-template-override-store`, `peer-config-store`, `runtime-composition` | Reuse unchanged. They continue to carry backend-neutral storage/configuration only. | Retain unchanged. | No runtime/daemon direct SQL edge and no new storage trait. |
| `request-dispatcher`, `daemon-request-dispatcher` | Reuse the existing canonical handler/dispatcher contract in AL.2; update a record only if the same implementation PR changes its actual code. | Retain only the one application dispatcher. | No adapter-specific dispatcher/decoder is added. |
| `post-send-hook-emitter`, `graft-post-send-port`, `atm-graft/post-send-notification-transport` | Replace/delete only with the accepted receiver-hook implementation in AL.1. | No old active record survives. | Tmux and graft are receiver implementations; sender-side nudge is forbidden. |
| `socket-server-transport`, `client-transport`, `server-transport`, `atm/local-socket-client-transport` | Leave historical records untouched while their legacy code is still live. | AM.2/AM.3 deletes or marks historical records in the same PR as their final code/dependency deletion. | No retained active record permits raw framing or a second local client/server. |
| `peer-http-adapter` | Leave unchanged until AL.7 proves its standard-runtime replacement; do not repurpose it as a peer application boundary. | AM.4 deletes it with the old peer client/listener/decoder code. | TLS physical authentication is recorded by the AL runtime facade, not a peer request protocol. |
| `peer-delivery-coordinator`, `post-commit-work-queue`, `post-write-router` | Do not extend these legacy delivery-state records. | AM.5 deletes each record with its corresponding scheduler/queue/post-write delivery code. | No timer, worker, cursor, replay, or sender-side hook boundary survives. |
| `atm-graft/shared-client-consumer`, non-transport config/doctor/status/inbox/notification manifests, concrete `atm-storage-rusqlite` manifests, and unrelated CLI manifests | No change. | No change, except removal of a record whose code is actually deleted by the ledger. | AL/AM may not use a transport rewrite to widen unrelated boundaries. |

## Boundary artifact acceptance rule

Every implementation PR that changes a listed boundary must include all of the
following in that **one** PR:

1. concrete code and tests;
2. the matching `boundaries/**.toml` creation/rename/retirement/deletion;
3. the matching human boundary document and crate re-export/allowlist update;
4. ADR-001 seal/edge review; and
5. a boundary-lint or mutation proof showing the new rule is enforced.

Conversely, a PR with only a boundary artifact is invalid for AL/AM. A plan
document is not a boundary artifact and cannot be used to pre-authorize a
future implementation.

## QA closure checks

- AL.1 checks that the only active hook manifest is
  `BOUNDARY-MessageReceivedHookEmitter` and its implementation list has the
  tmux receiver plus the separately owned graft receiver; daemon/runtime have
  no graft edge.
- AL.2/AL.4 checks that no new public transport trait or DTO boundary exists.
- AL.5–AL.7 checks that adapter code is behind the one runtime facade and uses
  unchanged public route types.
- AM.2–AM.5 checks each deleted module's manifest in the same diff and rejects
  an active manifest for removed raw framing, peer ingress, or replay code.
- AM.6 reconciles every path in the pinned baseline manifest inventory:
  retained records have a live, permitted owner; removed code has no active
  manifest; unrelated records were not widened. Any manifest added after the
  baseline is separately recorded with its introducing commit and disposition.
