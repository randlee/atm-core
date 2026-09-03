---
sprint: AW.4
title: "atm-graft-python observability paths and fallback satellite"
branch: feature/aw4-graft-fallback-observability
base: feature/aw2-sqlite-diagnostic-timeline
issues: "#904 (all items except the atm log merge, which AW.3 verifies)"
must_follow: [AW.1]
parallel_safe: [AW.1, AW.2]
---

# AW.4 — Graft fallback observability

> Branch base is AW.2 only for stack linearity; the code dependency is on
> AW.1's `atm-observability` path constants. Dev may start against AW.1's
> branch and rebase when AW.2 lands.

## Deliverables

1. **Rust-owned path diagnostics** on the Maturin binding
   (`crates/atm-graft-python`): `GraftSession.observability_paths() ->
   ObservabilityPaths { log_dir, canonical_log, fallback_log,
   resolved_from }` computed by `atm-observability`
   (`logger_root_for_log_dir` + AW.1's `graft_fallback_log_path`), with
   `resolved_from ∈ {"ATM_LOG", "default"}`. Python never receives a
   partial path and never computes one; `hermes-atm` calls this only for
   display.
2. **Fallback emitter**: a lazily-initialised, process-wide
   `GraftFallbackLogger` inside the binding (Rust), backed by
   `sc_observability::Logger` with its own `JsonlFileSink` at
   `fallback_log`, `Redactor` with the phase allowlist, rotation
   `GRAFT_FALLBACK_MAX_BYTES = 2 MiB`, `GRAFT_FALLBACK_MAX_FILES = 3`,
   queue capacity 256, `try_log` only. It is created on first use, never
   at import, so a healthy session with a reachable daemon touches no file.
   If the daemon's own observability is reachable (session connected), the
   binding **does not** duplicate into the fallback file — the satellite
   is for the daemon-unavailable window only.
3. **Event contract** (stable codes, `component = "atm_graft_python"`,
   `origin = "graft"`):
   - `ATM_GRAFT_DAEMON_UNAVAILABLE` — `action`, `endpoint_kind`
     (`unix|tcp|tls`), `error_layer` (from `AtmToolError.layer`),
     `correlation_id`;
   - `ATM_GRAFT_RECOVERY_ATTEMPT` — `strategy` (`refresh_only|retry_once`),
     `attempt`;
   - `ATM_GRAFT_RECOVERY_RESULT` — `outcome` (`recovered|failed`),
     `elapsed_ms`;
   - `ATM_GRAFT_FALLBACK_WRITE_FAILED` — emitted **only** to the envelope
     (item 4), never to the file that just failed.
   Emission points are `with_daemon_recovery` and `reconnect_client`
   (`crates/atm-graft-python/src/lib.rs`), plus the `AtmToolError`
   classification site in `tool_types.rs`.
4. **Envelope diagnostic**: every Python-facing result/error envelope gains
   an optional `observability` object: `{ "fallback_log": "<path>",
   "fallback_events": <n>, "fallback_write_failed": <bool>,
   "fallback_error_code": "<code|null>" }`, populated only when the
   fallback path was exercised for that call. Hermes can surface it
   without parsing files.
5. **Non-interference**: all emission is `try_log`; a fallback sink fault
   (unwritable dir, disk full via `RetainedSinkFaultInjector`) never changes
   the call's business result or raises into Python.
6. **Docs**: `docs/atm-core/modules/observability.md` gains a
   "Graft fallback satellite" section; the binding's Python stub (`.pyi`)
   documents `observability_paths()` and the envelope field.

## Contract samples

```python
>>> s = atm_graft.GraftSession(...)
>>> s.observability_paths()
ObservabilityPaths(log_dir='/Users/x/.atm/log', canonical_log='/Users/x/.atm/log/atm.log.jsonl', fallback_log='/Users/x/.atm/log/atm-graft-fallback.jsonl', resolved_from='default')
```

```json
{"ts":"2026-09-02T14:03:12.001Z","level":"error","component":"atm_graft_python","code":"ATM_GRAFT_DAEMON_UNAVAILABLE","origin":"graft","action":"send","endpoint_kind":"unix","error_layer":"transport","correlation_id":"c-8f1b","message":"daemon unavailable"}
```

Envelope on fallback write failure:

```json
{"ok":false,"error":{"code":"ATM_DAEMON_UNAVAILABLE","layer":"transport","recovery":"retry_once"},"observability":{"fallback_log":"/Users/x/.atm/log/atm-graft-fallback.jsonl","fallback_events":0,"fallback_write_failed":true,"fallback_error_code":"ATM_GRAFT_FALLBACK_WRITE_FAILED"}}
```

## Acceptance criteria

- AC1: `observability_paths()` returns absolute, fully resolved paths for
  both the `ATM_LOG` override and the default, and Python cannot alter
  them. (Closes #904 "canonical path diagnostics via bindings"; "hermes
  must not resolve paths".)
- AC2: Against a stopped daemon, one `send` produces exactly
  `DAEMON_UNAVAILABLE`, `RECOVERY_ATTEMPT`, `RECOVERY_RESULT(failed)` in
  the fallback file with shared `correlation_id`; against a daemon
  restarted mid-call, `RECOVERY_RESULT(recovered)`. (Closes #904 "record
  connection failure / recovery attempts / results".)
- AC3: Redaction: a `send` whose body, recipient, and env contain sentinel
  strings yields a fallback file containing none of the sentinels.
- AC4: With the fallback dir unwritable, the call result is identical to
  the writable run, Python sees no exception, and the envelope carries
  `fallback_write_failed = true` with the code. (Closes #904 "fallback
  write failure surfaced in envelope"; "never alter business outcome".)
- AC5: Rotation caps hold: writing 10 MiB of events leaves ≤ 3 files
  totalling ≤ 6 MiB + one record.
- AC6: A healthy connected session runs 100 calls and the fallback file is
  never created.
- AC7: The Python test suite passes on CPython 3.11, 3.12, 3.13, 3.14
  (the existing atm-graft-python CI matrix; extend the matrix if 3.14 is
  missing and note it in the PR).
- AC8: No file under `crates/atm-daemon/` is modified; `hermes-atm` (out of
  repo) needs no change beyond optionally calling the new API.

## Required validation

- `cargo test -p atm-graft-python`, `maturin develop` + `pytest` in the
  binding's test dir across the matrix.
- `just lint` with the boundary TOML updated for
  `atm-graft-python → atm-observability` (new edge; boundary-guard review).
- Manual: stopped daemon, one Python `send`, `cat atm-graft-fallback.jsonl`.

## Out of scope

- Merging the satellite into `atm log` (AW.3 verifies).
- Replay of failed sends; any Hermes-side logger.
