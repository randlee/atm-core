# Sprint AQ3 — Cross-Host Attachment Pull

Status: draft · Branch: `feature/aq-3-cross-host-pull` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Receiving daemon fetches attachment bytes per the AQ1 ADR decision (a),
verifies content address, then delivers the message.

Verified baseline: no byte-fetch, parked-message, dead-letter, or retry
machinery exists today — cross-host delivery is a stateless HTTPS push
(ADR-034/035) where receiver-side persistence *is* delivery. Every
deliverable below is new construction shaped by AQ1 ADR decisions (a) and
(f); this sprint implements those decisions, it does not re-open them.

## Deliverables

1. **Fetch mechanism** per ADR decision (a): the origin daemon serves
   content-addressed bytes over the authenticated peer channel (new peer
   HTTP endpoint or the justified fallback); the receiving daemon fetches
   into `attachment_dir()` and verifies sha256 and size.
2. **Pending-delivery semantics** per ADR decision (f): on an inbound
   envelope with attachments whose `origin_host` ≠ local host, the chosen
   mechanism (blocked inbound write, or parked not-yet-deliverable state
   hidden from the read surface) guarantees the recipient never observes an
   envelope whose attachments lack `local_path`. Includes the storage update
   path that sets `local_path` on the persisted envelope post-fetch.
3. **Failure semantics**: fetch or hash failure → message held/parked per
   decision (f) with an operator-visible structured log event naming
   `{msg_id, sha256, origin_host}`; retry only as scoped by the ADR (ADR-034
   rejects durable retry state — any retry here is the ADR's explicit
   extension, not an assumption); never a delivered envelope with
   missing/mismatched bytes.
4. **Dedupe**: a second envelope referencing an already-present `sha256` on
   the receiving host reuses bytes (hardlink or copy per ADR) — no refetch.
5. **Sender holds no transport state**: grep-gate (precedent:
   `scripts/check-legacy-mailbox-paths.py`) that no fetch/ssh/transport
   client code is reachable from `atm send`'s attach path.

## Normative fetch and storage boundary

AQ1 names the authenticated peer HTTP route and pending-delivery choice; AQ3
must implement that exact choice through one service boundary, not add a
second inbound write path:

```rust
pub struct AttachmentFetchRequest {
    pub sha256: AttachmentSha,
    pub size: u64,
    pub origin_host: HostName,
    pub origin_path: String,
}

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

The route is authenticated by ADR-034/035 peer context, serves only the
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
4. Grep-gate (deliverable 5) enumerated in CI.
5. `just test` all three CI lanes (ubuntu, macOS, Windows).

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
