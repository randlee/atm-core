---
sprint: AW.5
title: "hermes-atm native tool parity with atm CLI (atm_list / atm_read / atm_send)"
branch: feature/aw5-native-tool-parity
base: feature/aw4-graft-fallback-observability
issues: "#952 (all items)"
must_follow: [AW.4]
parallel_safe: [AW.3]
---

# AW.5 — Native tool parity with the atm CLI

> Added 2026-09-03 at team-lead's request. Sequenced after AW.4 because both
> sprints edit the same projection surface (`crates/atm-graft-python`
> result types and `crates/hermes-atm/src/hermes_atm/native_tools.py`
> `_send_result/_read_result/_list_result`); AW.4 lands the `observability`
> envelope field first so AW.5 extends one envelope shape, not two.

## Problem (from #952, verified 2026-08-19)

The three native tools return a strict subset of what `atm list --json`,
`atm read --json`, and `atm send --json` return:

- `atm_list`: row key `from_agent` vs CLI `from`; no envelope
  (`action`, `team`, `agent`, `selection_mode`, `history_collapsed`,
  `bucket_counts`); timestamps serialised `+00:00` vs CLI `Z`.
- `atm_read`: no `bucket_counts`; message body absent even though
  `mail_messages.message_text` holds it.
- `atm_send`: missing `action`, `team`, `agent`, `sender`, `summary`.

Rand's requirement: all three native tools return CLI-equivalent
information.

## Deliverables

1. **Single source of truth for the envelope.** The CLI's `--json`
   envelope for list/read/send is lifted into typed structs in `atm-core`
   (`ListEnvelope`, `ReadEnvelope`, `SendEnvelope`, serde-derived, with
   `bucket_counts`, `selection_mode`, `history_collapsed`, `action`,
   `team`, `agent`, `sender`, `summary`). `crates/atm/src/output.rs` and
   `commands/{list,read,send}.rs` serialise these structs instead of
   building JSON inline, so CLI output is unchanged byte-for-byte (golden
   test) and the binding cannot drift.
2. **Binding exposes the same structs.** `atm-graft-python` result types
   (`tool_types.rs`) gain the envelope fields via `#[pyo3(get)]`; the list
   row exposes `from` (keeping `from_agent` as a deprecated alias for one
   release, documented); timestamps are formatted through the CLI's
   formatter (`Z` suffix). `atm_read` returns the full message
   (`body`, `summary`, `source`, `timestamp`, `requires_ack`, `task_id`,
   `chat_id`) when the message is visible to the caller — the read path
   through atm-graft already resolves the row; if the daemon read route
   omits the body for chat-qualified reads, that gap is fixed in
   `atm-http-runtime` (Tokio/Axum path only).
3. **hermes-atm projections** rewritten as thin pass-throughs of the
   binding's typed results; no field renaming in Python.
4. **Parity test**: a fixture team with unread, pending-ack, and history
   messages is exercised through both `atm <cmd> --json` and the native
   tool; the JSON documents are compared key-for-key (allowing only the
   documented `observability` addition from AW.4 and the deprecated alias).

## Acceptance criteria

- AC1: For list/read/send over the parity fixture, `set(native.keys()) ⊇
  set(cli.keys())` and every shared key has an equal value; timestamps are
  byte-equal. (Closes #952 items 1–3.)
- AC2: `atm_read` returns the message body for a visible message; for a
  message outside the caller's scope both paths return `count: 0` with the
  same `bucket_counts`.
- AC3: CLI `--json` output for the same fixture is byte-identical before
  and after the refactor (golden files).
- AC4: `from_agent` alias emits a `DeprecationWarning` in Python once per
  process and is removed from the `.pyi` stub's documented surface.
- AC5: No legacy synchronous daemon change; any read-path fix lands in
  `atm-http-runtime` only.

## Required validation

- `cargo test -p atm-core -p atm -p atm-graft-python`; hermes-atm
  `pytest` across CPython 3.11–3.14.
- `just lint`; boundary TOML updated if `atm-graft-python` gains a new
  edge to `atm-core` envelope types (boundary-guard review).

## Out of scope

- New CLI fields; replay; any change to message scoping rules.
