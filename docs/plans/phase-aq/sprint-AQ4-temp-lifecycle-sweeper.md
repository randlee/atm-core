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

## Normative sweeper boundary

The daemon owns one supervised task; it is not a detached process and does not
walk arbitrary temporary directories:

```rust
pub struct AttachmentSweepConfig {
    pub interval: Duration,
    pub ttl: Duration,
    pub policy: SweepPolicy, // AQ1 ADR decision: ttl, on_ack, or both
}

pub struct SweepStats {
    pub scanned: u64,
    pub reclaimed_bytes: u64,
    pub skipped_foreign: u64,
}

pub async fn run_attachment_sweeper(
    cx: &DaemonContext,
    store: &dyn AttachmentSweepStore,
    config: AttachmentSweepConfig,
) -> Result<(), SweeperError>;
```

Each pass calls only `attachment_dir()`/the AQ1 path parser, refuses symlink
escapes, checks message acknowledgement/reference state through the storage
trait, emits one structured event with `SweepStats`, and updates the existing
health projection. Shutdown cancels and joins the task within the daemon
deadline.

## Acceptance criteria

1. Unit tests: expired dirs reclaimed; unexpired kept; foreign files skipped
   and logged; symlink escape attempt not followed.
2. Integration test: post-ack (or post-TTL, per policy) the msg dir is gone;
   a second message sharing the sha256 keeps its bytes until its own expiry.
3. Config defaults documented in the ADR appendix or daemon config docs.
4. `just test` all three CI lanes (ubuntu, macOS, Windows). Windows lane
   exercises the hardlink/symlink safety rails (no Unix-only assumptions in
   the layout scan).

## Paths to delete

None. AQ4 reclaims only expired/acknowledged AQ1 attachment directories; it
must not delete foreign files, active message data, or existing daemon/log
directories.

## Required validation

- `just test` workspace + daemon integration suite, ubuntu + macOS +
  Windows lanes.
- Evidence: one sweep log excerpt from a live daemon run committed on branch.

## Non-closure / out of scope

- Quota/size-pressure eviction beyond the ADR policy.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round so the sweeper
  sees the current message-id/staging behavior.
- parallel_safe: AQ3 only after AQ1's layout and policy are merged. AQ3 owns
  fetch/delivery while AQ4 owns the supervised sweep; both consume, never
  redefine, AQ1's `attachment_dir()`. AQ5 is parallel-safe because it owns
  shell/UI adapters only.
