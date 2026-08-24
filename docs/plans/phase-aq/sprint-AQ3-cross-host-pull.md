# Sprint AQ3 — Cross-Host Attachment Pull

Status: draft · Branch: `feature/aq-3-cross-host-pull` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Receiving daemon fetches attachment bytes per the AQ1 ADR decision (a),
verifies content address, then delivers the message.

Verified baseline: no byte-fetch, parked-message, dead-letter, or retry
machinery exists today — cross-host delivery is a stateless HTTPS push
(ADR-035 ingress under ADR-047 peer-wire security, single-router shape per
ADR-034) where receiver-side persistence *is* delivery. Every
deliverable below is new construction implementing AQ1 ADR decisions (a),
(f), and (i) exactly as recorded in ADR-054; this sprint re-opens none of
them, and deviations require an ADR change, not a sprint-local choice.

## Deliverables

1. **Fetch mechanism** per ADR decision (a): the origin daemon serves
   content-addressed bytes over the authenticated peer channel (new peer
   HTTP endpoint or the justified fallback); the receiving daemon fetches
   into `attachment_dir()` and verifies sha256 and size.
2. **Pending-delivery semantics**: implement AQ1's decision (f) exactly as
   written in ADR-054 — AQ3 makes no mechanism choice of its own. The
   decision-(f) default-candidate shape is: ordinary canonical write with
   `local_path: None` as an ordinary field state, fetch+verify strictly
   post-write, and gating only the read-surface projection until every
   attachment has a verified `local_path` — which is how the recipient never
   observes an envelope whose attachments lack `local_path`. If ADR-054
   instead recorded the non-default option (blocked inbound write or a
   hidden persistence state), AQ3's PR must cite the corresponding scoped
   amendment note in ADR-035/ADR-034 — it must not re-derive or re-justify
   the mechanism locally. Includes the storage update path that sets
   `local_path` on the persisted envelope post-fetch.
3. **Failure semantics**: fetch or hash failure → message held/parked per
   decision (f) with an operator-visible structured log event naming
   `{msg_id, sha256, origin_host}`; retry only as scoped by the ADR (ADR-034
   rejects durable retry state — any retry here is the ADR's explicit
   extension, not an assumption); never a delivered envelope with
   missing/mismatched bytes.
4. **Dedupe**: a second envelope referencing an already-present `sha256` on
   the receiving host reuses bytes exactly per ADR-054 decision (i) — the
   hardlink-vs-copy mechanism and its reference/link-count semantics are
   decided there, not here — no refetch. Any member/host address resolution
   this path needs reuses AQ2's canonical `resolve_picker_recipient`; AQ3
   must not define a second resolver.
5. **Single-owner grep-gate** (precedent:
   `scripts/check-legacy-mailbox-paths.py`), enumerated in CI, failing on:
   (a) fetch/ssh/transport client code reachable from `atm send`'s attach
   path; (b) any `attachment_dir`-shaped path construction outside AQ1's
   owner module; (c) any second member-address resolver outside AQ2's
   `resolve_picker_recipient`.
6. **Runtime hardening** (all per ADR-054 decision (a) bounds): server-side
   idle-read + total-transfer timeouts on the byte-fetch route; bounded
   in-flight fetch concurrency per origin host; daemon-side sha256/size
   verification and file copies run under `spawn_blocking` (or a bounded
   blocking pool), never inline on async workers; daemon shutdown cancels
   in-flight fetches within the shutdown deadline and removes (or leaves for
   AQ4's safety rails) partial staging files; a cumulative
   `attachment_fetch_failures_total` (and pending-count) counter on the
   daemon health surface; all new warn/error events carry `subsystem`,
   `action`, `outcome` structured fields per the ATM daemon logging advisory
   alongside `{msg_id, sha256, origin_host}`.

`AttachmentFetchError` inventory (variants normative):

| Variant | Cause | Outcome |
|---|---|---|
| `HashMismatch` | bytes fail sha256 verify | park + structured error |
| `SizeExceeded` | declared/actual size over ADR limit | park + structured error |
| `TransportOrAuth` | TLS/allowlist/HTTP failure | park; retry only per ADR bounds |
| `OriginUnreachable` | peer down/timeout | park; retry only per ADR bounds |

## Normative fetch and storage boundary

AQ1 names the authenticated peer HTTP route and pending-delivery choice; AQ3
must implement that exact choice through one service boundary, not add a
second inbound write path:

```rust
pub struct AttachmentFetchRequest {
    pub sha256: AttachmentSha,   // sole lookup key — content-addressed
    pub size: u64,
    pub origin_host: HostName,
    // origin_path deliberately absent (ADR-054): it lives only on the
    // persisted Attachment for display/audit and never reaches the fetch
    // boundary, so no implementation can misuse it as a lookup/path key.
}

// Dyn-dispatched (matching the phase's &dyn convention); methods are
// object-safe boxed-future async per the repo async-trait convention.
// PeerAttachmentSource is a transport-adapter trait owned by
// atm-http-runtime (outside the ADR-018 §3 storage-capability cap).
pub trait PeerAttachmentSource {
    async fn fetch(
        &self,
        request: AttachmentFetchRequest,
        deadline: Instant,
    ) -> Result<VerifiedAttachment, AttachmentFetchError>;
}

pub trait AttachmentDeliveryStore {
    async fn set_local_path(
        &self,
        message_id: AtmMessageId,
        attachment_index: usize,
        local_path: PathBuf,
    ) -> Result<(), StorageError>;
}
```

The route is authenticated by the ADR-035 ingress + ADR-047 peer-wire
context (mTLS default, plaintext test mode), serves only the
requested content-addressed bytes from the origin's registered staging root,
ignores `origin_path` as a filesystem instruction, enforces the declared size
limit, and returns no message or routing state. `set_local_path` is the only post-fetch
mutation; the read surface may expose the envelope only after every attachment
has a verified `local_path` under the AQ1-derived directory. The implementation
must document the concrete route and state transition in its PR by citing the
AQ1 ADR.

## Acceptance criteria

1. Two-daemon integration test (peer-pair harness precedent:
   `.just/tests/test_peer_pair_smoke.py` / `scripts/smoke/run_peer_pair.py`):
   cross-host send delivers with verified `local_path`; corrupted bytes at
   origin → held/parked message + structured error event, not delivery.
2. Dedupe test: two envelopes, one fetch (observable via fetch-count log
   event or filesystem inode check per ADR mechanism).
3. Ordering test: recipient read at any point never yields attachment refs
   without `local_path`.
4. Grep-gate (deliverable 5, all three prongs) enumerated in CI.
5. `just test` all three CI lanes (ubuntu, macOS, Windows).
6. Hardening tests (deliverable 6): stalled-peer fetch is cut by the
   server-side timeout; concurrency cap observed under multi-attachment
   fan-in; shutdown mid-fetch leaves no unreclaimable partial file; health
   counter increments on induced failure.

## Paths to delete

None. AQ3 adds the peer byte-fetch and pending-delivery path; it must not
delete or bypass canonical `WriteRequest`, ADR-034 authentication, or the
ordinary receiver read surface.

## Required validation

- `just test` + two-daemon integration suite, ubuntu + macOS + Windows CI
  lanes.
- One live cross-host demo (Mac ↔ second host) transcript committed as
  evidence, including an induced-failure run.
- Focused endpoint, hash/size, pending-state, and storage-update tests named
  in the PR and run without relying on a production peer.

## Non-closure / out of scope

- Sweeper/reclamation (AQ4). UI (AQ5). Team-level addressing.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round so the fetch
  path consumes the current CLI envelope and recipient contract.
- parallel_safe: AQ4 only after AQ1's layout/policy contract is merged. AQ3
  owns delivery/fetch and AQ4 owns reclamation; both call (never redefine)
  AQ1's `attachment_dir()`, and they must not share mutable implementation
  files. AQ5 is parallel-safe for CLI/UI surfaces only.
