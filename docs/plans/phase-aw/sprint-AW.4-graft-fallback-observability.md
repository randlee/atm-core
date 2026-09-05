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
3a. **`atm_ack` native tool** (required because #904-2 names it). It is a
   thin tool over the acknowledgement path that already exists in the
   binding: `PyGraftSession::send_tool(to=None, body=reply,
   requires_ack=false, acknowledges_message_id=Some(id))`
   (`lib.rs:522-537`). Additions: `PyGraftSession::ack_tool(message_id,
   reply)` delegating to that path; pydantic `AtmAckRequest { message_id,
   reply }` in `crates/atm-graft-python/python/atm_graft/models.py`; `AtmAckResult` = the same
   `AtmSendResult` type; hermes-atm `native_tools.py` `atm_ack` handler,
   schema entry and `register_tools` registration ("Acknowledge one ATM
   message that requires an ack"); `boundaries/atm-graft-python/hermes-graft-binding.toml`
   `request_types`/`response_types` updated. No new mailbox semantics.
3. **Event contract** emitted from the existing `with_daemon_recovery`
   classification path (`lib.rs:575-644`) for `atm_list`, `atm_read`,
   `atm_send` and `atm_ack`:
   - `ATM_GRAFT_DAEMON_UNAVAILABLE` — fields `endpoint_kind`,
     `failure_class`, `strategy`, `correlation_id`;
   - `ATM_GRAFT_RECOVERY_ATTEMPT` — `attempt`, `strategy`;
   - `ATM_GRAFT_RECOVERY_RESULT` — `outcome ∈ {recovered, failed}`,
     `elapsed_ms`;
   - `ATM_GRAFT_FALLBACK_WRITE_FAILED` — `error_layer`.
   `endpoint_kind` is sourced from
   `atm_daemon_client::local_daemon_transport()` and serialises as
   `unix_domain_socket | tcp_loopback` (the only variants of
   `LocalDaemonTransport`), exposed to the binding through a new
   `atm_graft::GraftClient::local_transport_label()` accessor so
   `atm-graft-python` adds no `atm-daemon-client` edge.
   `failure_class ∈ {stale_client, endpoint_unavailable}` is derived
   mechanically from the existing `DaemonRecovery` outcomes of
   `with_daemon_recovery`, which already make exactly the distinction
   #904-3 asks for:
   - `stale_client`: the first call failed `DaemonUnavailable` and
     `reconnect_client()` succeeded (`DaemonRecovery::Failed { refreshed:
     true, refresh_error: None }`, or `Completed` after the `RetryOnce`
     re-issue) — the endpoint was live, only the cached client was stale;
   - `endpoint_unavailable`: `reconnect_client()` itself failed
     (`DaemonRecovery::Failed { refresh_error: Some(_), .. }`) — the
     endpoint record could not be resolved or the daemon is not (yet)
     accepting; `refresh_error_code` carries the `AtmErrorCode` of the
     refresh failure so a startup race is inspectable without inventing a
     classifier the code does not have.
   No `daemon_starting` class is claimed.
4. **Envelope diagnostic**: `AtmSendResult/AtmReadResult/AtmListResult`
   (and the new ack result) gain an optional `observability` object
   `{ fallback_write_failed: bool, code?: str }`; hermes-atm passes it
   through untouched; the operation `outcome` is never altered by a logging
   failure.
5. **Boundary (definite)**: exactly one new Cargo edge,
   `atm-graft-python -> atm-observability`. Changes:
   `boundaries/atm-graft-python/hermes-graft-binding.toml`
   `allowed_dependencies` becomes `["atm-core", "atm-graft",
   "atm-observability", "pydantic"]`; `response_types` gains
   `AtmAckResult` and `PyObservabilityPaths`; `request_types` gains
   `AtmAckRequest` (final lists are stated verbatim in AW.5 D4);
   `.just/lint-config.toml` line 156 allowlist for
   `crates/atm-graft-python/Cargo.toml` gains `"atm-observability"`;
   `boundaries/atm-observability/tracing-bridge.toml` `allowed_dependents`
   gains `"atm-graft-python"` (this sprint, not AW.1, per the phase-plan
   invariant that grants land with the edge). `atm-graft` gains the
   `local_transport_label()` accessor within its existing
   `atm-daemon-client` dependency. hermes-atm (Python) gains no new
   dependency.
6. **Docs**: `docs/graft-observability.md` (event contract, paths, rotation
   isolation, envelope field).

## Acceptance criteria

- AC1 (904-1, 904-7 env resolution): `observability_paths()` returns
  Rust-derived paths; tests cover `ATM_LOG_DIR`, `ATM_HOME`, and default.
- AC2 (904-2, 904-3, 904-7): with the daemon stopped, each of
  `atm_list/atm_read/atm_send/atm_ack` writes one
  `ATM_GRAFT_DAEMON_UNAVAILABLE` record with `endpoint_kind` and
  `failure_class`; the existing `reconnect_replacement` test hook yields
  `stale_client`, a not-listening endpoint yields `endpoint_unavailable`
  with `refresh_error_code`; both enum values are covered.
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
- AC9: boundary lint passes with the deliverable-5 changes and the PR
  diff adds no Cargo dependency other than `atm-observability` to
  `crates/atm-graft-python/Cargo.toml`; no `crates/atm-daemon/` change.
- AC10 (atm_ack happy path): against a running daemon, `atm_ack` on a
  message that requires an ack returns `kind = "success"`, the message no
  longer appears under `atm_list(selection="pending_ack")`, and the result
  equals `atm ack <id> "<reply>" --json` key-for-key (same fixture as
  AW.5's parity test).
- AC11 (atm_ack errors): missing `reply` or `message_id` fails pydantic
  validation with the standard `invalid_request` envelope; acking a message
  that is not pending returns the same `AtmToolError` code the CLI reports
  (`atm ack` "not pending acknowledgement"), never a success envelope.
- AC12 (atm_ack exposure): `tool_schemas()` includes `atm_ack`;
  `register_tools` registers it; hermes-atm README lists it.

## Required validation

- `cargo test -p atm-graft-python`; `maturin develop` + `pytest
  crates/hermes-atm crates/atm-graft-python/tests` on the matrix.
- `just lint`; `grep -n 'sc-observability' Cargo.toml` shows `=1.2.0`
  unchanged.

## Out of scope

- Merged `atm log` view (AW.3); parity of list/read/send envelopes (AW.5).
