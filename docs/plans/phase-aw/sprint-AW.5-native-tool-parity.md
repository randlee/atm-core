---
sprint: AW.5
title: "hermes-atm native tool parity with atm CLI (#952)"
branch: feature/aw5-native-tool-parity
worktree: ../atm-core-worktrees/feature/aw5-native-tool-parity
status: complete
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
The binding (`crates/atm-graft-python/src/tool_types.rs`) redeclares
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
   `from` (`#[getter(from)]`) and keep `from_agent` as a
   deprecated alias emitting `DeprecationWarning` once per process.
2. **hermes-atm pass-through**: `_send_result/_read_result/_list_result`
   become `json.loads(result.to_json())`; the `kind/success` wrapper is
   unchanged. `atm_ack` (from AW.4 D3a) returns `AtmAckResult`, an alias of
   `AtmSendResult`, so it takes the same `to_json()` path and is covered by
   the parity test (D3, AC6) — the claim is verified here, not inherited.
   This sprint
   edits only the paths reserved to it in the phase plan §4 table; the
   parity test drives the built `atm` binary and never edits `crates/atm/`. README documents
   the `from_agent` deprecation and the parity guarantee.
3. **Parity test**: for a fixture mailbox, run the CLI (`atm list --json`,
   `atm read --json`, `atm send --json`, `atm ack <id> "<reply>" --json`)
   and the native tools (`atm_list`, `atm_read`, `atm_send`, `atm_ack`)
   against the same daemon; assert key-for-key equality of the result
   objects (ignoring the hermes `kind` wrapper and the AW.4 `observability`
   field). The ack case sends a `requires_ack` message from a second
   fixture identity, acks it natively, and compares against the CLI ack of
   a twin message. The test
   lives in `crates/atm-graft-python/tests/test_cli_parity.py` and runs in
   CI's platform test job on Python 3.14.7. The abi3 compatibility matrix
   independently validates wheel imports on Python 3.11–3.14.
4. **Boundary (definite)**: the binding adds its explicitly allowlisted
   `serde_json` dependency so each `to_json()` method uses the canonical
   serializer directly. The baseline this sprint diffs against is the
   post-AW.4 record (`must_follow: [AW.4]`):
   `boundaries/atm-graft-python/hermes-graft-binding.toml`
   `allowed_dependencies = ["atm-core", "atm-graft", "atm-observability",
   "pydantic", "serde_json"]`; the matching `.just/lint-config.toml`
   manifest allowlist is updated for `crates/atm-graft-python/Cargo.toml`.
   The record's
   `[contracts].response_types` becomes exactly
   `["PyMessage", "PyMailboxWorkCounts", "PyNudge", "AtmSendResult",
   "AtmAckResult", "AtmReadResult", "AtmListRow", "AtmListResult",
   "PyObservabilityPaths"]` — the existing wrapper names are retained (they
   now wrap `SendOutcome`/`ReadOutcome`/`ListOutcome` rather than
   redeclaring fields), `AtmAckResult` and `PyObservabilityPaths` are the
   AW.4 additions (AW.4 D3a/D5 commit them first; this sprint re-asserts
   the final list). `request_types` is unchanged from AW.4's final list and
   is exactly `["PyAgentAddress", "PyGraftSessionOptions",
   "AtmSendRequest", "AtmAckRequest", "AtmReadRequest", "AtmListRequest"]`.
   Nothing else in the record changes.

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
- AC5: no change to list/read/send/ack scoping rules or CLI output
  (cli_surface and existing CLI JSON snapshot tests unchanged); boundary
  lint passes with the D4 `allowed_dependencies`, `request_types` and
  `response_types` lists verbatim; no `crates/atm-daemon/` change.
- AC6 (952-E1; atm_ack parity): the parity test's ack case passes —
  `atm_ack(...).to_json()` equals `atm ack <id> "<reply>" --json`
  key-for-key after canonical JSON normalisation, and both leave the
  message absent from `pending_ack` listings.

## Required validation

- `cargo test -p atm-graft-python -p atm-core`; `pytest` on 3.11–3.14.
- `just lint`; `grep -n 'sc-observability' Cargo.toml` shows `=1.2.0`
  unchanged.

## Out of scope

- New fields not present in the CLI output; scoping-rule changes.
