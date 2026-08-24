# Sprint AQ4 — ATM_TEMP Sweeper

Status: draft · Branch: `feature/aq-4-atm-temp-sweeper` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Daemon-owned periodic reclamation of `$ATM_TEMP` per AQ1 decision (b):
TTL-only, 30 days, everything under the root. A shared well-known folder
with no owner is a guaranteed leak; with `ATM_TEMP` defined at the system
level, this one sweeper covers every ATM feature.

## Deliverables

1. **Sweeper task in the daemon runtime**: periodic Tokio task following the
   retained-log maintenance-worker precedent
   (`crates/atm-daemon/bin_support/daemon_observability.rs`, 60 s cadence),
   removing any entry directly under `$ATM_TEMP` whose mtime (newest file
   within, for directories) exceeds the TTL. TTL (default 30 d) and sweep
   interval from daemon config with documented defaults. Reads the validated
   `AtmTemp` from AQ1 startup resolution — never the env var directly. Pure
   filesystem + config: **no storage traits, no ack coupling**.
2. **Safety rails**: sweeps only entries under the resolved `$ATM_TEMP`
   root; never follows symlinks out of the root; a root that disappears
   mid-sweep aborts the pass with a structured warning, not a crash.
   Filesystem work runs under `spawn_blocking`/a bounded blocking pool,
   never inline on the async worker.
3. **Observability**: per-sweep structured log event `{scanned,
   reclaimed_bytes, skipped}` plus the mandatory `subsystem`, `action`,
   `outcome` fields (ATM daemon logging advisory), and a cumulative counter
   on the health report following the `queue_full_drops_total` precedent.
   Recorded exception (reviewed): the event is emitted through the existing
   `emit_daemon_event` composition helper for consistency with the
   retained-log worker precedent; this reuse is an explicit, scoped
   exception to the tracing-facade preference and does not authorize
   refactoring `emit_daemon_event` itself.

## Normative sweeper boundary

```rust
pub struct AtmTempSweepConfig {
    pub interval: Duration,
    pub ttl: Duration, // default 30 days (AQ1 decision (b))
}

pub struct SweepStats {
    pub scanned: u64,
    pub reclaimed_bytes: u64,
    pub skipped: u64,
}

pub async fn run_atm_temp_sweeper(
    cx: &DaemonContext,
    atm_temp: &AtmTemp,
    config: AtmTempSweepConfig,
) -> Result<(), SweeperError>;
```

`SweeperError` semantics: per-entry failures (busy file, permission,
symlink escape) are skip-and-log, never `Err`; `Err` is reserved for
pass-fatal conditions (root unavailable), surfaced on the health report.
Shutdown cancels and joins the task within the daemon deadline.

## Acceptance criteria

1. Unit tests: expired entries reclaimed; unexpired kept; symlink escape not
   followed; per-entry failure skips and logs without failing the pass.
2. Integration test: entry older than TTL is gone after a sweep; a fresh
   entry survives. (Zero-interval/zero-TTL rejection is an AQ1 decision-(a)
   startup-validation deliverable with its own AQ1 AC — verified there.)
3. Config defaults (30 d TTL, sweep interval) documented in the ADR appendix
   or daemon config docs.
4. `just test` all three CI lanes (ubuntu, macOS, Windows); Windows lane
   exercises the symlink/junction rail.

## Paths to delete

None. AQ4 reclaims only expired entries under `$ATM_TEMP`; it must not touch
anything outside the resolved root.

## Required validation

- `just test` workspace + daemon integration suite, ubuntu + macOS +
  Windows lanes.
- Evidence: one sweep log excerpt from a live daemon run committed on branch.

## Non-closure / out of scope

- Quota/size-pressure eviction. On-ack reclamation (rejected by AQ1
  decision (b) — TTL-only).

## Dependencies

- must_follow: AQ1 (consumes `AtmTemp` and decision (b)) — merge-forward
  before every dev/fix round. AQ2 not required.
- parallel_safe: AQ2, AQ3, AQ5 (disjoint modules; sweeper owns only the
  daemon task).
