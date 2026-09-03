---
sprint: AW.4
title: "Graft fallback observability on the atm-graft-python binding"
branch: feature/aw4-graft-fallback-observability
base: integrate/phase-aw
issues: "#904 ids 904-1, 904-2, 904-3, 904-5, 904-6, 904-7, 904-8"
must_follow: [AW.1]
parallel_safe: [AW.2]
---

# AW.4 — Graft fallback observability

## Deliverables

1. **Path diagnostics API** (Rust, `crates/atm-graft-python/src/lib.rs`):
   `#[pyfunction] observability_paths() -> PyObservabilityPaths { log_dir,
   canonical_log_path, fallback_log_path, log_dir_source:
   "env:ATM_LOG_DIR"|"env:ATM_HOME"|"default" }` derived from the same
   resolver the daemon uses (`atm-observability` path helpers +
   `graft_fallback_log_path` from AW.1). hermes-atm never computes a path.
2. **`GraftFallbackLogger`** (Rust, `atm-graft-python`): appends redacted
   JSONL to `atm-graft-fallback.jsonl`; own rotation
   (`GRAFT_FALLBACK_MAX_BYTES = 2 MiB`, `GRAFT_FALLBACK_KEEP_FILES = 3`)
   isolated from the daemon's `atm.log.jsonl` rotation; bounded in-process
   queue `GRAFT_FALLBACK_QUEUE = 256` with `try_send`; write failures are
   captured into the call's envelope (deliverable 4) and never raise.
   Records carry `origin = "graft"` and only `RETAINED_FIELD_ALLOWLIST`
   fields.
3. **Event contract** emitted from the existing `with_daemon_recovery`
   classification path (`lib.rs:433-644`) for `atm_list`, `atm_read`,
   `atm_send`, **and `atm_ack`** (the ack tool call added here to the
   session API, mirroring `send_tool` with `acknowledges_message_id`):
   - `ATM_GRAFT_DAEMON_UNAVAILABLE` — fields `endpoint_kind`,
     `failure_class`, `strategy`, `correlation_id`;
   - `ATM_GRAFT_RECOVERY_ATTEMPT` — `attempt`, `strategy`;
   - `ATM_GRAFT_RECOVERY_RESULT` — `outcome ∈ {recovered, failed}`,
     `elapsed_ms`;
   - `ATM_GRAFT_FALLBACK_WRITE_FAILED` — `error_layer`.
   `endpoint_kind` is sourced from
   `atm_daemon_client::local_daemon_transport()` and serialises as
   `unix_domain_socket | tcp_loopback` (the only variants of
   `LocalDaemonTransport`). `failure_class ∈ {stale_client,
   endpoint_unavailable, daemon_starting}` is set by the recovery
   classifier: `stale_client` when the first call on a cached client fails
   and `reconnect_client()` then succeeds; `endpoint_unavailable` when the
   endpoint cannot be reached after reconnect; `daemon_starting` when the
   endpoint accepts but health reports not-ready. Together they satisfy
   904-3.
4. **Envelope diagnostic**: `AtmSendResult/AtmReadResult/AtmListResult`
   (and the new ack result) gain an optional `observability` object
   `{ fallback_write_failed: bool, code?: str }`; hermes-atm passes it
   through untouched; the operation `outcome` is never altered by a logging
   failure.
5. **Boundary (definite)**: `boundaries/atm-graft-python/hermes-graft-binding.toml`
   `allowed_dependencies` gains `"atm-observability"` and
   `"atm-daemon-client"` (if not already present) and `response_types` gains
   `PyObservabilityPaths`; `.just/lint-config.toml` manifest allowlist for
   `crates/atm-graft-python/Cargo.toml` gains the same crates. hermes-atm
   (Python) gains no new dependency.
6. **Docs**: `docs/graft-observability.md` (event contract, paths, rotation
   isolation, envelope field).

## Acceptance criteria

- AC1 (904-1, 904-7 env resolution): `observability_paths()` returns
  Rust-derived paths; tests cover `ATM_LOG_DIR`, `ATM_HOME`, and default.
- AC2 (904-2, 904-3, 904-7): with the daemon stopped, each of
  `atm_list/atm_read/atm_send/atm_ack` writes one
  `ATM_GRAFT_DAEMON_UNAVAILABLE` record with `endpoint_kind` and
  `failure_class`; a stale-client fixture yields `stale_client` and a
  not-listening endpoint yields `endpoint_unavailable`.
- AC3 (904-2 redaction): records never contain body, recipient, chat_id,
  token, env values (fixture substring test).
- AC4 (904-6, 904-7): with the fallback file unwritable, the envelope shows
  `observability.fallback_write_failed = true` and the operation `outcome`
  is unchanged.
- AC5 (904-5): fallback rotation keeps ≤ 3 files of ≤ 2 MiB and never
  opens/renames `atm.log.jsonl`.
- AC6 (904-5): the binding never writes to `atm.log.jsonl` (test: daemon
  file inode/size unchanged across 1000 fallback events).
- AC7 (904-7 concurrency): daemon running + 4 concurrent graft sessions
  under induced daemon restart produce no lost primary outcomes and no
  interleaved/corrupt fallback lines.
- AC8 (904-8): binding tests green on Python 3.11, 3.12, 3.13, 3.14 (CI
  matrix).
- AC9: boundary lint passes with the deliverable-5 changes; no
  `crates/atm-daemon/` change.

## Required validation

- `cargo test -p atm-graft-python`; `maturin develop` + `pytest
  crates/hermes-atm crates/atm-graft-python/tests` on the matrix.
- `just lint`; `grep -n 'sc-observability' Cargo.toml` shows `=1.2.0`
  unchanged.

## Out of scope

- Merged `atm log` view (AW.3); parity of list/read/send envelopes (AW.5).
