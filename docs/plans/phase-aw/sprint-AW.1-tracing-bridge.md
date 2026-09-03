---
sprint: AW.1
title: "Tracing bridge into sc-observability and runtime stderr retirement"
branch: feature/aw1-tracing-bridge
base: integrate/phase-aw
issues: "#905 ids 905-1, 905-2, 905-8 (fields/filtering/recursion/non-interference/redaction), 905-9 (allowlist doc)"
must_follow: []
parallel_safe: []
---

# AW.1 — Tracing bridge into sc-observability

## Deliverables

1. **`TracingBridgeLayer`** in `crates/atm-observability` (new module
   `tracing_bridge.rs`; no new crate). A `tracing_subscriber::Layer` that:
   - forwards `tracing` events at `WARN` and `ERROR` unconditionally, and
     `INFO` events whose target starts with an entry of
     `RETAINED_INFO_TARGETS`, a `&[&str]` constant whose initial value is
     `["atm_daemon_bootstrap::lifecycle", "atm_http_runtime::listener",
     "atm_storage_rusqlite::maintenance"]`; the sprint moves the daemon's
     start/stop/bind/unbind/checkpoint `info!` sites onto those explicit
     `target:` values (test asserts each target has at least one emitter);
   - converts each event into a `sc_observability::LogEvent` via a
     `tracing::field::Visit` visitor: `target` → `component`, the message
     → `message`, and every other field kept only if its name is in
     `RETAINED_FIELD_ALLOWLIST` (phase plan §2);
   - applies the reserved `origin` rule from the phase plan §2 "No
     recursion": `origin = "tracing"` unless the event carries its own
     `origin` field;
   - never blocks: uses `Logger::try_log` and counts `QueueFull` in
     `TracingBridgeStats { forwarded_total, dropped_queue_full_total,
     dropped_reentrant_total }` (atomics, `Arc`-shared for AW.3);
   - is reentrancy-guarded with a thread-local flag so an event raised while
     the layer is emitting (including any `tracing` call inside
     sc-observability itself) is counted and discarded, not recursed;
   - exposes `fn set_diagnostic_sink(&self, sink: Arc<dyn DiagnosticSink>)`
     using the `DiagnosticSink` / `RetainedEvent` / `SinkOffer` contract
     defined verbatim in the phase plan §2 "Cross-sprint contracts"
     (implemented in AW.2); called after JSONL emission, skipped when
     `origin ∈ {"sqlite", "timeline"}`; `Dropped` counted in
     `TracingBridgeStats::sink_dropped_total`;
   - implements `atm_core::observability_counters::DiagnosticCountersSource`
     (struct and trait added by this sprint to `atm-core`, phase plan §2)
     for `TracingBridgeStats` so health output (AW.3) needs no new crate edge.
2. **Installation in the daemon** (`crates/atm-daemon-bootstrap`): after
   `DaemonObservability` is constructed, install the layer as the process
   global subscriber (`tracing_subscriber::registry().with(layer)`), exactly
   once; a second install attempt returns `BridgeError::AlreadyInstalled`
   and emits one diagnostic. The layer takes the same `Logger` handle the
   daemon already owns; no second logger, no second file.
3. **Runtime stderr retirement.** The two live-runtime `eprintln!` sites
   (`crates/atm-daemon-bootstrap/src/lib.rs:520,524`) become `tracing::info!`
   / `tracing::error!` events with `target: "atm_daemon_bootstrap::lifecycle"`.
   New lint gate `scripts/check-runtime-stderr.py` (registered in
   `.just/run_lint.py` and the `justfile`): non-test `eprintln!`/`println!`
   in `crates/{atm-daemon-bootstrap,atm-http-runtime,atm-runtime}/src` are
   forbidden outside `PRE_BOOTSTRAP_STDERR_ALLOWLIST` (a list of
   `file:function` entries in the script, initially the log-dir-unwritable
   and observability-init-failed paths that run before a logger exists).
   The allowlist is reproduced in `docs/atm-daemon/logging.md`.
4. **Shared path constants** in `crates/atm-observability`:
   `CANONICAL_LOG_FILE_NAME = "atm.log.jsonl"` and
   `GRAFT_FALLBACK_LOG_FILE_NAME = "atm-graft-fallback.jsonl"`, plus
   `pub fn graft_fallback_log_path(log_dir: &Path) -> PathBuf`. AW.4
   consumes these; nothing else in AW.1 touches graft.
5. **Doc alignment**: `docs/atm-daemon/logging.md` "default retained event
   set" section rewritten to describe what is actually bridged (levels,
   `RETAINED_INFO_TARGETS`, `RETAINED_FIELD_ALLOWLIST`, loss semantics
   under `QueueFull`, the stderr allowlist). The stale version text at
   `logging.md:118` ("`sc-observability 1.0.0`") and
   `docs/requirements.md:907` (`sc-observability = "1.0.0"`) is corrected to
   the pinned `=1.2.0`.
6. **Boundary record (definite, enforceable).** New
   `boundaries/atm-observability/tracing-bridge.toml` with:
   - `owner_package = "atm-observability"`, facade `TracingBridgeLayer`;
   - `allowed_dependents = ["atm-daemon-bootstrap", "atm"]` — the two
     crates that depend on `atm-observability` today (`atm` for path
     helpers). No grant to `atm-graft-python` here; AW.4 amends this record
     in the sprint that adds that Cargo edge;
   - `[dependencies].allowed_dependencies` = the exact dependency set of
     `crates/atm-observability/Cargo.toml` after this sprint: today
     `atm-core`, `sc-observability-types` plus the additions
     `sc-observability`, `tracing`, `tracing-subscriber` (the PR must show
     the TOML and the manifest agree; `lint_boundaries.py` fails otherwise);
   - `forbidden_edges = ["atm-daemon -> atm-observability",
     "atm-observability -> atm-daemon-bootstrap",
     "atm-observability -> atm-http-runtime",
     "atm-observability -> atm-storage-rusqlite"]` — all crate-level, the
     only form `collect_forbidden_edge_violations` enforces. The first entry
     is true today (`crates/atm-daemon/Cargo.toml` depends only on
     `atm-daemon-bootstrap`) and keeps the frozen daemon from ever linking
     the bridge directly;
   - the module-level rule "only `atm-daemon-bootstrap` installs the layer"
     is enforced by a new source-scan test in
     `crates/atm-architecture/tests/boundary_enforcement.rs`
     (`tracing_bridge_is_installed_only_by_daemon_bootstrap`) asserting
     `TracingBridgeLayer::install` appears in
     `crates/atm-daemon-bootstrap/src/` and in no other crate's `src/`.
   Plus the new `[[boundaries.manifest_dependency_allowlists]]` entry in
   `.just/lint-config.toml` with `owner_manifest_path =
   "crates/atm-observability/Cargo.toml"`, `boundary_record_path =
   "boundaries/atm-observability/tracing-bridge.toml"`, and the same
   `allowed_dependencies` list. boundary-guard reviews both.

## Contract samples

Retained JSONL record produced from
`tracing::warn!(target: "atm_http_runtime::delivery", code = "ATM_DELIVERY_RETRY", correlation_id = %cid, attempt = 3, "delivery retry scheduled")`:

```json
{"ts":"2026-09-02T14:03:11.412Z","level":"warn","component":"atm_http_runtime::delivery","code":"ATM_DELIVERY_RETRY","correlation_id":"c-8f1a","attempt":3,"message":"delivery retry scheduled","origin":"tracing"}
```

A field named `body`, `recipient`, `token`, `env`, or any name outside the
allowlist is absent from the record even if present on the event.

## Acceptance criteria

- AC1 (905-1, 905-8 fields): with the daemon running via
  `atm-daemon-bootstrap`, an induced `tracing::warn!` and `tracing::error!`
  from each of `atm-http-runtime`, `atm-runtime`, `atm-storage-rusqlite`
  appears in `atm.log.jsonl` with `level`, `component`, `code`,
  `correlation_id`, `message`, `origin`.
- AC2 (905-8 filtering): an `INFO` event from a target outside
  `RETAINED_INFO_TARGETS` is not retained; one from each listed target is.
- AC3 (905-8 recursion): a `tracing` event emitted from inside the layer's
  own emission path (test: a `LogSink` whose `write` calls
  `tracing::warn!`) increments `dropped_reentrant_total` and does not
  recurse or deadlock (test bounded by a 5 s timeout).
- AC4 (905-8 non-interference): with a `RetainedSinkFaultInjector`-saturated
  queue, the bridge returns immediately, `dropped_queue_full_total`
  increases, and a real `send` through the HTTP runtime completes with an
  unchanged result.
- AC5 (905-8 redaction): a fixture event carrying `body`, `recipient`,
  `token`, `env` values produces a record with none of those keys and none
  of the values as substrings anywhere in the line.
- AC6 (905-2, 905-9): no non-test `eprintln!`/`println!` remains in the
  three runtime crates outside the documented allowlist; the gate fails on
  a synthetic violation (test in `scripts/tests/`); `logging.md` lists the
  allowlist; `grep -n '1\.0\.0' docs/atm-daemon/logging.md
  docs/requirements.md` returns nothing.
- AC7: layer installation is idempotent (`BridgeError::AlreadyInstalled`).
- AC8: `boundaries/atm-observability/tracing-bridge.toml` exists with the
  D6 contents, `python3 .just/lint_boundaries.py` passes, the manifest
  allowlist entry matches `crates/atm-observability/Cargo.toml` exactly,
  and the `boundary_enforcement.rs` source-scan test passes (and fails on a
  synthetic `TracingBridgeLayer::install` call placed in another crate).
- AC9: `DiagnosticSink`, `RetainedEvent`, `SinkOffer`, `DropReason` and
  `DiagnosticCounters`/`DiagnosticCountersSource` exist with the signatures
  in the phase plan §2 (doc-test compiles them).
- AC10: no file under `crates/atm-daemon/` is modified.

## Required validation

- `cargo test -p atm-observability -p atm-daemon-bootstrap -p atm-core -p atm-architecture`
- `just lint` (boundaries, function-length, new runtime-stderr gate).
- `grep -n 'sc-observability' Cargo.toml` shows both `=1.2.0` lines
  unchanged; PR diff contains no change to them.
- Manual smoke: start daemon, trigger a delivery failure against a stopped
  peer, show the retained record with `atm log --level warn`.

## Out of scope

- SQLite persistence (AW.2), `atm log` changes (AW.3), graft emitter (AW.4).
- CLI `--stderr-logs` behaviour (unchanged).
