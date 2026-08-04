---
title: Phase AK Plan — direct peer HTTP delivery
status: proposed
branch: plan/mvp-simplification
worktree: ../atm-core-worktrees/plan/mvp-simplification
target: integrate/phase-ak
---

# Phase AK — direct peer HTTP delivery

## Goal

Replace daemon-owned peer workers, custom DNS, and custom mTLS transport with
one direct HTTP request function. A host-qualified write uses the same request
shape as local HTTP. The receiving daemon persists it and emits the ordinary
nudge.

Phase AK starts only after Phase AI merges to `develop`, on
`integrate/phase-ak`. It does not alter the already-planned Phase AJ
runtime-observation work. AK.2's temporary no-delivery state must never merge
to `develop`; AK.4 restores and proves direct delivery on this phase branch.
AK.7 is an independent daemon-launch hardening sprint: it has no peer-routing,
transport, retry, or lifecycle-state dependency and may merge on its own QA
result.

## Documentation transition

The current requirements and ADRs describe the superseded HTTPS worker design;
they must not be silently contradicted by implementation. AK.2 exclusively
marks the worker/replay portion of `REQ-CORE-TRANSPORT-003B` and ADR-038
superseded, and retires only Phase AI worker claims in project/architecture/
boundary text; it defines no resend replacement semantics. AK.3 exclusively
owns the alias/configuration subclauses of `REQ-CORE-TRANSPORT-002A/-002D`,
ADR-040, and the corresponding admission language in ADR-035. AK.4 owns the
active direct-delivery subclauses (`REQ-CORE-TRANSPORT-002`, `-002B`, `-002B1`,
`-002C`, `-004`, and `-005A`) and creates ADR-047; it must not revise
AK.3's alias semantics. AK.5 creates ADR-046 and exclusively defines the new
resend-cache semantics for `REQ-CORE-TRANSPORT-003/-003B`. AK.8 owns the
one-request `messages[]` peer-receive contract; AK.9 owns its sender use,
atomic outbound confirmation, and resend-state simplification. AK.6 finalizes
supersession markers and removes obsolete wording without changing AK.3's
alias contract or AK.8/AK.9 active semantics. AK.10 closes the final
post-write-router boundary contract after AK.9, and AK.11 is a separate,
post-AK.6 physical M5 proof sprint. No sprint may claim a code/doc mismatch as
an acceptable interim state after its own PR completes.

AK.7 implements the existing `REQ-P-RUNTIME-006` launch-environment boundary:
every standard daemon launcher strips `ATM_TEAM`, `ATM_IDENTITY`, and
`ATM_ENVIRONMENT` before `atm-daemon` starts, and daemon production code has
no ambient caller-context fallback. It updates only daemon-launch/
daemon-boundary documentation; it neither changes Phase AJ caller-context
semantics nor adds agent-state behavior.

## MVP contract

| Concern | Decision |
| --- | --- |
| Peer configuration | Canonical full hostname plus explicit aliases and port. Configuration changes populate a small alias index. SQLite stores the full hostname, never a resolved IP. |
| Wire | HTTP/1.1 `POST /v1/atm/messages`. A peer singleton and a peer `messages[]` array normalize at the existing receiver boundary into the same canonical write admission path. One peer array request receives one success only after the entire array is durably accepted; it is not HTTP keep-alive framing of independent writes. |
| Security | Explicit trusted-LAN MVP: no mTLS, certificate pinning, or claimed-source authentication. The sender-host header is display provenance only. AK.4 refuses wildcard or multicast peer-listener binds and binds only configured local interface addresses; network isolation beyond that explicit bind allowlist remains a deployment/firewall responsibility, not an ATM-enforced Internet-exposure guarantee. |
| Receiver | AK.4 creates the minimal plain `PeerHttpListenerSet` after AK.2 removes `HttpsListenerSet`. AK.8 extends its existing HTTP decoder to accept peer `messages[]`, normalize each item into the canonical write admission path, and make the full accepted array durable before one response. Post-write nudges remain best-effort after commit and never change receive success. |
| Sender | CLI and graft use their existing local daemon HTTP call. After canonical SQLite persistence, the daemon makes one private batch request: an immediate delivery carries one message and a recovered page carries its ordered array. One successful response atomically retires that exact array's durable outbound markers. `config` is the immutable configured source-host snapshot, never CLI input or a per-send peer scan. |
| AK.4 baseline | No automatic retry. A failed direct send returns an ordinary delivery error and leaves the admitted SQLite record undelivered. |
| AK.5 resend cache | Adds optional per-endpoint `Connected`/`Disconnected`/`Queued` state and one timer. `peer_resend_cache = true` is the default; `false` preserves AK.4's no-retry behavior. AK.9 flattens the redundant one-field aggregate without changing the timer's bounded state ownership. |
| Failure | Ordinary typed send failure; no receipt synthesis, remote mailbox mutation, or local nudge fallback. |

Every sprint inventories its important structs, enums, traits, and execution
boundaries. An unlisted new type, trait, thread, task, worker, channel,
listener, persistence table, or route requires a plan amendment before code.

The plan intentionally does not invent a second cross-host protocol, a curl
subprocess, client-side SQLite ownership, a client-only payload, or a
local/cross-host inbound branch. `curl` is the executable proof for this
ordinary HTTP request; the production call uses the same HTTP request/response
contract in-process.

## Required removal

Delete, not adapt:

- `crates/atm-daemon/src/peer_drain_coordinator.rs` and its composition,
  shutdown, tests, `PeerDeliveryCoordinator`, per-message threads, job state,
  and peer delivery post-commit queue key.
- `crates/atm-daemon/src/peer_resolution.rs` and literal-IP authority
  discovery in `runtime_health/peer_authority.rs`.
- The legacy custom TLS module in AK.2. AK.6 begins independently from the
  pre-AK.2 Phase AK baseline and preserves verified TLS provisioning/
  configuration and curl-interoperable receiver support only in
  `crates/atm-peer-tls-interop`, an isolated, unused TLS crate.
  No daemon, CLI, graft, or active send-path crate may depend on it. It is not a native
  peer transport: no native ATM TLS sender is known to work. It does not
  preserve the failing native outbound client in active code.
- Worker-only peer recovery/replay observability, policies, and documentation.

The existing simple-send path is intentionally reduced in explicit steps:

| Current step | AK owner | Result |
| --- | --- | --- |
| Save the immutable write locally. | Retain | Canonical SQLite admission remains first. The origin record retains its immutable write plus destination host; this is durable message data, not worker state. |
| Queue only `{ hostname, message_id }`. | AK.2 | Delete. |
| Start a coordinator thread. | AK.2 | Delete. |
| Start a per-message thread. | AK.2 | Delete. |
| Re-scan SQLite for the write just saved. | AK.2 | Delete. AK.4 uses the in-memory `WriteRequest` for an immediate send. |
| Read every trusted-peer row. | AK.3 | Replace with the O(1) immutable alias-to-full-host index; delete the old broad scan. |
| Resolve every peer hostname for literal-IP alias discovery. | AK.3 | Replace with explicit configured IP aliases; delete inferred discovery. |
| Start a DNS thread for the selected peer. | AK.3 | Delete. Resolve the persisted full hostname only when connecting; never in an ATM DNS thread. |
| Open custom TCP/rustls HTTP. | AK.2/AK.4 | AK.2 deletes the legacy stack; AK.4 creates the plain direct-HTTP replacement. |
| Receive a successful peer response. | AK.9 | One `MessageStore::confirm_peer_delivery_batch` transaction removes the exact accepted array's `peerOutbound` markers; accepted writes do not re-enter AK.5's backlog. |

Retain unchanged unless a direct compile consumer proves otherwise:

- `WriteRequest`, origin ULID idempotency, `ResponseEnvelope`, HTTP framing,
  body limits, canonical persistence, receiver ACK transition, and ordinary
  post-write nudge.
- Host-qualified address parsing and the exact host/port peer alias row.

## Sprint validation index

This is a navigation aid, not a second acceptance-criteria source. Each
sprint's `Explicit prohibitions` and `Required validation` sections are
authoritative:

- AK.1: salvage ledger and curl provenance/ACK/nudge proof.
- AK.2: deletion ledger and compiler-attributed worker removal proof.
- AK.3: alias persistence and staged curl receiver/nudge proof.
- AK.4: direct production send/receiver/nudge chain and bind control.
- AK.5: cache-disabled, immediate, due-batch, restart, and nudge proofs.
- AK.8: one-request peer `messages[]` decoding, canonical atomic receive, and non-fatal post-commit nudge proof.
- AK.9: singleton/page sender use of that request, whole-array confirmation, and flattened resend-state proof.
- AK.10: final post-write-router boundary/source comparison and executable three-route contract proof.
- AK.6: isolated pre-AK.2 curl-mTLS fixture and final documentation evidence.
- AK.11: independent, retained physical M5 cross-host proof after AK.6 merges.
- AK.7: daemon launch-environment sanitation and ambient-context boundary
  proof.

`atm-peer-tls-interop` is a preservation boundary, not a service component:
it may contain provisioning/configuration value objects and a curl-mTLS
receiver interoperability fixture. It exposes no daemon listener, outbound
sender, routing API, background work, or production dependency edge.

## Verification sequence

Every sprint runs `just lint`, `just test`, `just smoke localhost`, and
`just smoke local-ip` against an isolated test home/database. AK.3 also runs
the bidirectional current-configured `crosshost-curl-tls` receiver/nudge proof,
because it intentionally has no production sender after AK.2. AK.4 converts
the standard `crosshost-curl-plain` lane to its configured production listener;
AK.4, AK.5, AK.8, AK.9, and AK.6 each then run it with bidirectional
`crosshost-send` and `crosshost-ack` on M4/M5. AK.11 runs after the AK.6 merge
and is the only sprint that may close the outstanding disabled-cache-first
physical proof finding: it retains an independently reviewable M5 evidence
bundle, including the induced failure/no-retry case. Every successful receiver
case must prove one remote read, exact ULID/body, host-qualified rendering where
applicable, and exactly one nudge; every rejected/truncated/failed case must
prove no false confirmation or nudge. Curl remains independent protocol
evidence, never a substitute for the production-sender proof.

## Sprint order

| Sprint | Closure | Dependencies | Recommended agent |
| --- | --- | --- | --- |
| AK.1 | Recover cross-host ACK/provenance from `fix/crosshost-ack-provenance`: audit, retain the useful fixes, and prove remote curl message/ACK/nudge behavior. | Must follow Phase AI merge to `develop`; lands only on `integrate/phase-ak`. | arch-ctm |
| AK.2 | Delete the daemon peer worker and all worker-only state. | Must follow AK.1 development push and merge-forward. AK.1 PR must merge before AK.2 PR completion. | arch-ctm |
| AK.3 | Normalize configured aliases to full hostnames before persistence, with no outbound delivery behavior change; prove canonical ingress/nudge with curl. | Must follow AK.2 development push and merge-forward. AK.2 PR must merge before AK.3 PR completion. | arch-ctm |
| AK.4 | Prove direct full-host HTTP delivery with no retry, including one production sender/receiver/nudge chain. | Must follow AK.3 development push and merge-forward. AK.3 PR must merge before AK.4 PR completion. | arch-ctm |
| AK.5 | Add optional default-on resend caching through AK.4's proven HTTP function, including restart recovery and disabled-cache behavior. | Must follow AK.4 development push and merge-forward. AK.4 PR must merge before AK.5 PR completion. | arch-ctm |
| AK.8 | Replace peer HTTP keep-alive frame loops with the one-request `messages[]` receive contract while retaining the canonical inbound admission/nudge path. | Must follow AK.5 development push and merge-forward. AK.5 PR must merge before AK.8 PR completion. | arch-ctm |
| AK.9 | Send immediate and recovered arrays through AK.8's contract, atomically retire their backlog markers, and flatten the resend aggregate wrapper. | Must follow AK.8 development push and merge-forward. AK.8 PR must merge before AK.9 PR completion. | arch-ctm |
| AK.6 | Independently preserve provisioning/receiver curl-mTLS interop from the pre-AK.2 baseline in an inactive crate. | May develop in parallel from the pre-AK.2 `integrate/phase-ak` baseline after the Phase AI→`develop` entry gate. Before PR completion, merge-forward the current integration head and rerun its own validation; it does not wait for AK.8/AK.9. | Cipher-311d |
| AK.10 | Resolve `AK5-BOUNDARY-DRIFT-001` with a direct comparison of the final post-write router and its boundary record, then lock the three route outcomes with an executable guard. | Must follow the AK.6 merge and AK.9 development push/merge-forward. AK.6 and AK.9 PRs must merge before AK.10 PR completion. | arch-ctm |
| AK.11 | Resolve `AK5-CROSSHOST-PROOF-001` with a dedicated disabled-cache-first physical M5 proof bundle. | Starts only after AK.6 is merged to `integrate/phase-ak`; it is an independent evidence sprint, not an AK.6 smoke subtask. | arch-ctm + M5 operator |
| AK.7 | Strip caller identity/team/environment from every daemon launch and remove daemon ambient-caller reads. | May develop and merge independently after the Phase AI→`develop` entry gate. Its owned launch/boundary paths do not overlap AK.1–AK.6. | Cipher-311d |

Each dependent sprint begins immediately after its predecessor development is
pushed and merge-forwarded; it does **not** wait for predecessor QA approval.
Every dependent development or fix round first merges its predecessor. A
dependent PR cannot complete before its predecessor PR merges.

AK.6 is the explicit parallel exception: Cipher may start its isolated
pre-AK.2-baseline interop fixture immediately after
the Phase AI entry gate, without waiting for AK.5, AK.8, or AK.9. Before final
validation, any final fix round, and PR completion, it merges the current
integration head; it must never restore active legacy TLS code to the post-AK.2
line.

AK.10 begins after the AK.6 merge and AK.9 development push. It merges both
accepted prerequisites before final validation and PR completion. AK.11 is
deliberately not folded into AK.6: it begins only after AK.6 merges to
`integrate/phase-ak`, from that accepted line and with a real M5 peer. Its
immutable evidence bundle is the closure artifact for
`AK5-CROSSHOST-PROOF-001`; a unit test, a localhost result, or an ignored
ad-hoc smoke report cannot close that finding.

AK.7 is also parallel-safe: Cipher may start from the `integrate/phase-ak`
baseline immediately after the Phase AI entry gate and merge as soon as AK.7
QA passes. It does not wait for, merge-forward, or modify AK.1–AK.6. If the
shared baseline advances before an AK.7 fix round, merge it first; resolve only
the AK.7-owned launch/boundary files.

## Governing changes

AK.1 records and implements the surviving cross-host provenance fixes. AK.2
marks only the worker/replay portion of `REQ-CORE-TRANSPORT-003B` and ADR-038
superseded, then updates project/architecture/boundary text to retire
worker-specific AI.28/AI.31/AI.32 claims; AK.5 later owns the replacement
resend-cache semantics. AK.8/AK.9 amend ADR-047 and the active direct-delivery
requirements to define one request/one response batch delivery and one atomic
outbound confirmation. AK.10 corrects the post-write-router boundary contract
and its route/error outcomes against that final implementation. AK.6 completes
ADR-047's supersession of
ADR-034/040/041 and the corresponding requirements/boundary rules: peer host
alias configuration remains; custom TLS/pinning, inferred literal-IP authority
discovery, and daemon worker delivery do not. ADR-035 remains the canonical
single-ingress/nudge rule.

AK.7 implements and makes testable `REQ-P-RUNTIME-006` plus the existing
daemon-ambient-identity prohibition in `REQ-CORE-CONFIG-001`. It updates
`docs/atm-daemon-client/boundaries.md`, `docs/atm-daemon/requirements.md`, and
the daemon startup documentation to make the shared CLI/graft auto-start
boundary explicit. No new ADR is needed: this is enforcement of the accepted
runtime requirement, not a new architectural option.
