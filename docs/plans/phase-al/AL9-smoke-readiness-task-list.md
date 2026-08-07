# AL.9 smoke-readiness task list

**Status:** active implementation checklist.  A check is complete only when its
targeted tests pass.  It does not authorize activating or altering an ambient
daemon; live evidence remains an operator-owned AL.9 gate.

This list supersedes AL.9's earlier source-only proof status for the purpose of
preparing smoke execution.  All new work is in the Tokio/Axum
`atm-http-runtime` line; `atm-daemon` remains a thin replacement-runtime
executable and the frozen legacy daemon implementation is out of scope.

## SR-002 — Localhost and same-IP route-to-hook proof

- [ ] Add an isolated runtime smoke harness that exercises `localhost` and
      the selected loopback address through the public typed client and
      records one durable write and one received-hook result per new message.
- [ ] Include a repeated same-ID case proving no second received hook, and a
      hook failure/timeout case proving durable success plus warning.
- [ ] Ensure the proof reads the same canonical response schema for UDS and
      loopback; no raw/legacy dispatcher is permitted.

## SR-003/SR-004 — Cross-host transport and physical proof

**Deferred — not an AL.9 implementation item.** The replacement runtime
remains limited to authenticated loopback TCP and Unix UDS. AL.9 must not add
an unencrypted non-loopback listener or a second peer client merely to force
an M5 proof. A future secure connector assignment must first define its
authentication and listener boundary; only then may it reuse the existing
typed client and canonical Axum route. Until that assignment exists, the M5
row is intentionally dropped from AL.9 closure and no cross-host smoke runner
is shipped from this branch.

## SR-006 — Live-proof execution prerequisites

- [ ] Update the physical-proof matrix and benchmark gate with the exact
      commands, required clean OS-user/backup authority, expected evidence,
      team-lead-owned rollback/park behavior, and separate macOS and Windows
      rows.
- [ ] Run all focused Rust and Python readiness tests, then `just fmt`,
      `just lint`, `just test`, and `git diff --check` at the final SHA.
- [ ] Send the final commit and an AL.9 review request to team-lead and
      quality-mgr.  Do not represent static/integration proof as an authorized
      host activation.

## Non-negotiable acceptance rules

- One typed HTTP encoder/decoder and one canonical Axum route process every
  currently supported local write.
- Storage is durable before a received hook; hook error or timeout returns
  success plus warning and never becomes a write failure.
- A duplicate message ID is informational and does not emit another hook.
- No transport switches silently after a failed request, and one direct
  failure creates no retry/replay state.
- A real live smoke is executed only in an isolated environment and never by
  hijacking an ambient daemon.
