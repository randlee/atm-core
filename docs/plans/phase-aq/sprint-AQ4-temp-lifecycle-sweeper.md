# Sprint AQ4 — Attachment Temp Lifecycle Sweeper

Status: draft · Branch: `feature/aq-4-sweeper` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Daemon-owned reclamation of `<known-temp>/atm/` per the AQ1 ADR policy
decision (d). A shared well-known folder with no owner is a guaranteed leak.

## Deliverables

1. **Sweeper task in the daemon runtime**: periodic scan of
   `attachment_dir()` roots applying the ADR policy (TTL / on-ack / both).
   Interval and TTL from daemon config with documented defaults.
2. **Safety rails**: sweeper deletes only paths matching the AQ1 layout
   derivation; anything else under the root is logged and left. Never follows
   symlinks out of the root.
3. **Observability**: per-sweep log line `{scanned, reclaimed_bytes,
   skipped_foreign}` and a counter surfaced through existing daemon metrics.
4. **Dedupe interaction**: content still referenced by an unswept msg-id is
   not reclaimed (refcount or link-count check per AQ1/AQ3 mechanism).

## Acceptance criteria

1. Unit tests: expired dirs reclaimed; unexpired kept; foreign files skipped
   and logged; symlink escape attempt not followed.
2. Integration test: post-ack (or post-TTL, per policy) the msg dir is gone;
   a second message sharing the sha256 keeps its bytes until its own expiry.
3. Config defaults documented in the ADR appendix or daemon config docs.
4. `just test` both CI lanes.

## Required validation

- `just test` workspace + daemon integration suite, macOS + Windows lanes.
- Evidence: one sweep log excerpt from a live daemon run committed on branch.

## Non-closure / out of scope

- Quota/size-pressure eviction beyond the ADR policy.

## Dependencies

- must_follow: AQ2 — merge-forward before every dev/fix round.
- parallel_safe: AQ3 (sweeper task vs delivery/fetch path; both consume
  AQ1's layout, neither redefines it). AQ5 parallel_safe.
