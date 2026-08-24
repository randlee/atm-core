# Sprint AQ4 — Attachment Temp Lifecycle Sweeper

Status: draft · Branch: `feature/aq-4-sweeper` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Daemon-owned reclamation of `<known-temp>/atm/` per the AQ1 ADR policy
decision (d). A shared well-known folder with no owner is a guaranteed leak.

## Deliverables

1. **Sweeper task in the daemon runtime**: periodic Tokio task following the
   retained-log maintenance-worker precedent
   (`crates/atm-daemon/bin_support/daemon_observability.rs`, 60 s cadence),
   scanning `attachment_dir()` roots and applying the ADR policy (TTL /
   on-ack / both). On-ack state is queried from
   `mail_message_states.acknowledged_at`
   (`crates/atm-storage-rusqlite/src/shared_db.rs`). Interval and TTL from
   daemon config (`AtmConfig`, key added by AQ1 decision (e)) with
   documented defaults.
2. **Safety rails**: sweeper deletes only paths matching the AQ1 layout
   derivation; anything else under the root is logged and left. Never follows
   symlinks out of the root.
3. **Observability**: per-sweep structured log event `{scanned,
   reclaimed_bytes, skipped_foreign}` via the existing daemon event surface
   (`emit_daemon_event`) and a cumulative counter exposed through the health
   report, following the `queue_full_drops_total` precedent — the daemon has
   no metrics registry.
4. **Dedupe interaction**: content still referenced by an unswept msg-id is
   not reclaimed (refcount or link-count check per AQ1/AQ3 mechanism).

## Acceptance criteria

1. Unit tests: expired dirs reclaimed; unexpired kept; foreign files skipped
   and logged; symlink escape attempt not followed.
2. Integration test: post-ack (or post-TTL, per policy) the msg dir is gone;
   a second message sharing the sha256 keeps its bytes until its own expiry.
3. Config defaults documented in the ADR appendix or daemon config docs.
4. `just test` all three CI lanes (ubuntu, macOS, Windows). Windows lane
   exercises the hardlink/symlink safety rails (no Unix-only assumptions in
   the layout scan).

## Required validation

- `just test` workspace + daemon integration suite, ubuntu + macOS +
  Windows lanes.
- Evidence: one sweep log excerpt from a live daemon run committed on branch.

## Non-closure / out of scope

- Quota/size-pressure eviction beyond the ADR policy.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round.
- parallel_safe: AQ3 (sweeper task vs delivery/fetch path; both consume
  AQ1's layout, neither redefines it). AQ5 parallel_safe.
