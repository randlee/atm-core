---
sprint: AW.5
title: "hermes-atm native tool parity with atm CLI (#952)"
branch: feature/aw5-native-tool-parity
base: integrate/phase-aw
issues: "#952 ids 952-E1, 952-E2, 952-E3, evidence items 1–3"
must_follow: [AW.4]
parallel_safe: [AW.3]
---

# AW.5 — Native tool parity with the CLI

## Problem (corrected)

The CLI `--json` output is the serialisation of the canonical `atm-core`
outcome structs (`ListOutcome` at `crates/atm-core/src/commands/list.rs:113`,
`ReadOutcome` at `commands/read/mod.rs:60`, `SendOutcome` at
`commands/send/outcome.rs:15`), written by `crates/atm/src/output.rs`.
The binding (`crates/atm-graft-python/src/tool_types.rs`) re-declares
reduced result types and hermes-atm `native_tools.py` projects them again.
The message body **is** already returned by `atm_read` when a message is
selected (`PyMessage::from_read` → `_message_result`). The real gaps are:

- item 1: list rows use `from_agent`; CLI uses `from`;
- item 2: no envelope (`action`, `team`, `agent`, `selection_mode`,
  `history_collapsed`, `bucket_counts`; `sender`/`summary` on send);
- item 3: timestamps serialise as `+00:00` (chrono `to_rfc3339`) instead of
  the CLI's `Z`;
- read: message metadata (`summary`, `timestamp`, `requires_ack`,
  `task_id`, `chat_id`) and the `bucket_counts` envelope are missing.

## Deliverables

1. **Expose the canonical outcomes.** No new struct family: the binding's
   `AtmListResult/AtmReadResult/AtmSendResult` wrap the `atm-core`
   `ListOutcome/ReadOutcome/SendOutcome` values and expose every
   serialised field via pyo3 getters, plus `to_json() -> str` produced by
   the same `serde_json` path `output.rs` uses (so timestamp formatting,
   key names and enum spellings are shared by construction). Rows expose
   `from` (`#[pyo3(name = "from")]`) and keep `from_agent` as a
   deprecated alias emitting `DeprecationWarning` once per process.
2. **hermes-atm pass-through**: `_send_result/_read_result/_list_result`
   become `json.loads(result.to_json())`; the `kind/success` wrapper is
   unchanged. `atm_ack` (from AW.4) uses the same path. README documents
   the `from_agent` deprecation and the parity guarantee.
3. **Parity test**: for a fixture mailbox, run the CLI (`atm list --json`,
   `atm read --json`, `atm send --json`) and the native tools against the
   same daemon; assert key-for-key equality of the result objects (ignoring
   the hermes `kind` wrapper and the AW.4 `observability` field). The test
   lives in `crates/atm-graft-python/tests/test_cli_parity.py` and is in the
   3.11–3.14 matrix.
4. **Boundary (definite)**: no new crate edge. `atm-graft-python` already
   depends on `atm-core` (`boundaries/atm-graft-python/hermes-graft-binding.toml`
   `allowed_dependencies = ["atm-core", "atm-graft", "pydantic"]`; manifest
   allowlist at `.just/lint-config.toml` for
   `crates/atm-graft-python/Cargo.toml` already lists it). The record's
   `[contracts].response_types` is updated to name the wrapped outcome
   types; nothing else changes.

## Acceptance criteria

- AC1 (952-E1, E2, E3; items 1–3): parity test passes for list, read, send
  with byte-identical `to_json()` vs CLI `--json` (after canonical JSON
  normalisation), including `from`, envelope fields, `bucket_counts`,
  `sender`, `summary`, and `Z` timestamps.
- AC2 (952-E2): `atm_read` for a selected message returns the full message
  object (body plus metadata) exactly as the CLI does; for a message that
  is not visible to the caller under the read selection rules (wrong
  recipient/team/chat scope), the native result equals the CLI's
  not-found outcome (same `outcome`/`count`), never a partial object.
- AC3 (item 3): timestamps in every native result end with `Z`.
- AC4 (item 1): `row.from_agent` still works and emits one
  `DeprecationWarning`; `row.from` is the documented field.
- AC5: no change to list/read/send scoping rules or CLI output (cli_surface
  and existing CLI JSON snapshot tests unchanged); boundary lint passes;
  no `crates/atm-daemon/` change.

## Required validation

- `cargo test -p atm-graft-python -p atm-core`; `pytest` on 3.11–3.14.
- `just lint`; `grep -n 'sc-observability' Cargo.toml` shows `=1.2.0`
  unchanged.

## Out of scope

- New fields not present in the CLI output; scoping-rule changes.
