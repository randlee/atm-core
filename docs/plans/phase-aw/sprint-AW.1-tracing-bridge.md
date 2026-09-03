---
sprint: AW.1
title: "Tracing bridge into sc-observability and runtime stderr retirement"
branch: feature/aw1-tracing-bridge
base: integrate/phase-aw
issues: "#905 (items: tracing bridge, structured-field preservation, eprintln retirement, retained-set doc alignment)"
must_follow: []
parallel_safe: [AW.4]
---

# AW.1 — Tracing bridge into sc-observability

## Deliverables

1. **`TracingBridgeLayer`** in `crates/atm-observability` (new module
   `tracing_bridge.rs`, behind the crate's existing public surface; no new
   crate). A `tracing_subscriber::Layer` that:
   - forwards `tracing` events at `WARN` and `ERROR` unconditionally, and
     `INFO` events whose target is in an explicit allowlist
     (`RETAINED_INFO_TARGETS`: daemon lifecycle, endpoint bind/stop,
     storage maintenance) — the retained set is a constant, not a filter
     string;
   - converts each event into a `sc_observability::LogEvent` preserving
     structured fields via a `tracing::field::Visit` visitor into
     `LogEvent::fields` — `level`, `target` (as `component`), `code` (when a
     `code` field is present), `correlation_id`, `action`, `outcome`,
     `elapsed_ms`, and the event message as `message`;
   - applies the sink's `Redactor` before emission, and additionally drops
     any field whose name is not in `RETAINED_FIELD_ALLOWLIST` (see redaction
     invariant in the phase plan);
   - never blocks: uses `Logger::try_log` and counts `QueueFull` in an
     `AtomicU64` exposed through `TracingBridgeStats { forwarded_total,
     dropped_queue_full_total, dropped_reentrant_total }`;
   - is reentrancy-guarded with a thread-local flag so an event raised while
     the layer is emitting (including any `tracing` call inside
     sc-observability itself) is counted and discarded, not recursed.
2. **Installation in the daemon** (`crates/atm-daemon-bootstrap`): after
   `DaemonObservability` is constructed, install the layer as the process
   global subscriber (`tracing_subscriber::registry().with(layer)`), exactly
   once; a second install attempt is a logged diagnostic, not a panic. The
   layer takes the same `Logger` handle the daemon already owns; no second
   logger, no second file.
3. **Runtime stderr retirement.** The two live-runtime `eprintln!` sites
   (`crates/atm-daemon-bootstrap/src/lib.rs:520,524`) become `tracing::info!`
   / `tracing::error!` events with `component = "daemon.lifecycle"`. A
   workspace grep gate (`just lint` addition in `.just/`): non-test
   `eprintln!`/`println!` in `atm-daemon-bootstrap`, `atm-http-runtime`,
   `atm-runtime` are forbidden outside an explicit
   `PRE_BOOTSTRAP_STDERR_ALLOWLIST` (fatal errors raised before a logger can
   exist, e.g. log-dir unwritable). The allowlist is documented in
   `docs/atm-daemon/logging.md`.
4. **Shared path constants** in `crates/atm-observability`:
   `CANONICAL_LOG_FILE_NAME = "atm.log.jsonl"` (already implied by
   `prepare_retained_log`) and `GRAFT_FALLBACK_LOG_FILE_NAME =
   "atm-graft-fallback.jsonl"`, plus `fn graft_fallback_log_path(log_dir)`.
   AW.4 consumes these; nothing else in AW.1 touches graft.
5. **Doc alignment**: `docs/atm-daemon/logging.md` "default retained event
   set" section rewritten to describe what is actually bridged (levels,
   info allowlist, field allowlist, loss semantics under `QueueFull`).

## Contract samples

Retained JSONL record produced from
`tracing::warn!(target: "atm_http_runtime::delivery", code = "ATM_DELIVERY_RETRY", correlation_id = %cid, attempt = 3, "delivery retry scheduled")`:

```json
{"ts":"2026-09-02T14:03:11.412Z","level":"warn","component":"atm_http_runtime::delivery","code":"ATM_DELIVERY_RETRY","correlation_id":"c-8f1a","attempt":3,"message":"delivery retry scheduled","origin":"tracing"}
```

Field named `body`, `recipient`, `token`, `env`, or any name outside the
allowlist is absent from the record even if present on the event.

## Acceptance criteria

- AC1: With the daemon running via `atm-daemon-bootstrap`, an induced
  `tracing::warn!` and `tracing::error!` from each of `atm-http-runtime`,
  `atm-runtime`, `atm-storage-rusqlite` appears in `atm.log.jsonl` with
  `level`, `component`, `code`, `correlation_id`, `message`, `origin`.
  (Closes #905 "bridge tracing → sc-observability" and "preserve
  structured fields".)
- AC2: An `INFO` event from a non-allowlisted target is not retained; an
  `INFO` event from an allowlisted target is.
- AC3: A `tracing` event emitted from inside the layer's own emission path
  (test: a `LogSink` whose `write` calls `tracing::warn!`) increments
  `dropped_reentrant_total` and does not recurse or deadlock (test bounded
  by a 5 s timeout).
- AC4: With a `RetainedSinkFaultInjector`-saturated queue, the bridge
  returns immediately, `dropped_queue_full_total` increases, and the
  calling request path completes with an unchanged result
  (non-interference test on a real `send` through the HTTP runtime).
- AC5: Fields outside `RETAINED_FIELD_ALLOWLIST` never appear in the
  record; a fixture event carrying `body`, `recipient`, `token`, `env`
  values produces a record with none of those keys and none of the values
  as substrings anywhere in the line.
- AC6: No non-test `eprintln!`/`println!` remains in the three runtime
  crates outside the documented allowlist; the lint gate fails on a
  synthetic violation (test in the `.just` script's own test file).
- AC7: Layer installation is idempotent; a second install returns
  `Err(BridgeError::AlreadyInstalled)` and emits one diagnostic.
- AC8: No file under `crates/atm-daemon/` (legacy sync daemon) is modified.

## Required validation

- `cargo test -p atm-observability -p atm-daemon-bootstrap`
- `just lint` (boundary lints unchanged — the layer lives in the crate that
  already owns sc-observability; if `atm-observability` must depend on
  `tracing-subscriber`, update `boundaries/atm-observability/*.toml`
  explicitly and call it out in the PR body for boundary-guard).
- `scripts/check-function-length.py` clean.
- Manual smoke: start daemon, trigger a delivery failure against a stopped
  peer, show the retained record with `atm log --level warn`.

## Out of scope

- SQLite persistence (AW.2), `atm log` changes (AW.3), graft emitter (AW.4).
- CLI `--stderr-logs` behaviour (unchanged).
