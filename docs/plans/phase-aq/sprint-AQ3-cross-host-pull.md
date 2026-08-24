# Sprint AQ3 — Cross-Host Attachment Pull

Status: draft · Branch: `feature/aq-3-cross-host-pull` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Receiving daemon fetches attachment bytes per the AQ1 ADR decision (a),
verifies content address, then delivers the message.

## Deliverables

1. **Fetch step in the receiving daemon's delivery path**: on an envelope
   with attachments whose `origin_host` ≠ local host, fetch into
   `attachment_dir()`, verify sha256 and size, set `local_path`, then
   deliver. Delivery order guarantee: the recipient never observes an
   envelope whose attachments lack `local_path`.
2. **Failure semantics**: fetch or hash failure → message parked with an
   operator-visible error naming `{msg_id, sha256, origin_host}`; retry
   policy per ADR; never a delivered envelope with missing/mismatched bytes.
3. **Dedupe**: a second envelope referencing an already-present `sha256` on
   the receiving host reuses bytes (hardlink or copy per ADR) — no refetch.
4. **Sender holds no transport state**: grep-gate that no fetch/ssh/transport
   client code is reachable from `atm send`'s attach path.

## Acceptance criteria

1. Two-daemon integration test (loopback hosts): cross-host send delivers
   with verified `local_path`; corrupted bytes at origin → parked message +
   error, not delivery.
2. Dedupe test: two envelopes, one fetch (observable via fetch-count metric
   or filesystem inode check per ADR mechanism).
3. Ordering test: recipient read at any point never yields attachment refs
   without `local_path`.
4. Grep-gate (deliverable 4) enumerated in CI.
5. `just test` both CI lanes.

## Required validation

- `just test` + two-daemon integration suite, macOS + Windows CI lanes.
- One live cross-host demo (Mac ↔ second host) transcript committed as
  evidence, including an induced-failure run.

## Non-closure / out of scope

- Sweeper/reclamation (AQ4). UI (AQ5). Team-level addressing.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round.
- parallel_safe: AQ4 gated on disjoint modules — AQ3 owns the delivery/fetch
  path, AQ4 owns the sweeper task; both consume (never redefine) AQ1's
  `attachment_dir()`. AQ5 parallel_safe (CLI/UI surfaces only).
