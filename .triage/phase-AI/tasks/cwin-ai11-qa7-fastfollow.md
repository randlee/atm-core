# AI.11 QA-7 fast-follow — deletion gate scope-widening (3 findings, all blocking)

Branch: `feature/pAI-s11-post-merge-remediation` (this branch — already checked out for you)
File: `crates/atm-architecture/tests/boundary_enforcement.rs`
Test: `ai11_deletion_gate_rejects_retired_windows_transport_ast_and_dependencies()` (~line 353)

This is the ONLY thing blocking the AI.11 merge right now. No code changes have
landed on this branch since `bb20fd9a` — none of these three findings are fixed yet
despite an earlier note in the `.ttl` triage records claiming a partial fix. Treat
all three as fully open.

## Finding 1 — AI11-QA7-ARCH-001-HARDCODED-ALLOWLIST

`guarded_sources` (line ~375) hardcodes exactly 5 files and only scans those via
`retired_windows_transport_ast_findings()`. Two sibling guards in the SAME file
already do a full workspace sweep instead of a fixed allowlist:
- `production_code_cannot_restore_retired_error_contract_symbols()`
- `workspace_source_must_not_reintroduce_retired_peer_delivery_constructs()`

Both call `collect_rust_files(&workspace_root().join("crates"), &mut files)` and
scan every workspace Rust file.

**Fix**: widen `guarded_sources` to a full `collect_rust_files` workspace sweep,
following the exact pattern already used by those two sibling guards. Do not
touch `AI11-QA5-ATM-QA-002-DELETION-GATE-TEST.ttl` — that finding is already
closed and must not be reopened.

## Finding 2 — AI11-QA7-ARCH-002-TAUTOLOGICAL-DUPLICATE-CHECK

Two sub-checks in the same test function are each scoped to a single
already-known file, so neither can ever catch a real regression:

(a) Duplicate-router check (line ~447): `dispatcher_source.matches("impl ApiRouter for").count()`
against exactly one hardcoded file (`crates/atm-daemon/src/runtime_health.rs`) —
the count is tautologically always 1.

(b) Adapter storage/nudge-call check (line ~423-440): forbidden-list scan
(`LocalServiceRuntime`, `persist_message`, `emit_post_send_effects`,
`write_mail_with_runtime`) only scans `local_tcp_source`
(`crates/atm-daemon/src/local_tcp_transport.rs`) — one of three production
transport adapters. `local_ipc_transport.rs` (Unix UDS) and `https_transport.rs`
(cross-host HTTPS) are never scanned.

**Fix**: widen both sub-checks to scan across all relevant adapter files (or the
full workspace sweep, consistent with Finding 1's fix), not just one hardcoded
file each.

## Finding 3 — AI11-QA7-ATM-QA-201-ENVELOPE-CODEC-COVERAGE

The AI.11 sprint doc requires the deletion gate to reject six retired construct
families: pipe, Windows AF_UNIX, generic envelope wire codec, duplicate router,
non-loopback bind, adapter storage/nudge calls. The test currently covers five —
there is zero coverage for the retired generic HTTP `RequestEnvelope`/
`ResponseEnvelope` wire-codec pattern.

**Fix**: add a sub-check (or dedicated assertion) that rejects reintroduction of
the generic envelope wire-codec constructs, following the same AST/text-scan
pattern already used by the other five sub-checks in this test.

## Notes

- All three findings are gate-hardening only — there is no active production
  violation today. The risk is purely the gate's inability to catch a *future*
  regression. No production code changes are expected; scope is limited to
  `boundary_enforcement.rs`.
- When done: commit, push this branch, and report back (branch + SHA) so this
  can be triaged for merge into `integrate/phase-AI`.
