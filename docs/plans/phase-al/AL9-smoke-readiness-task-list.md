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

## SR-003 — Direct non-TLS cross-host transport

- [ ] Define validated replacement-runtime peer configuration: explicit bind
      address, exact configured remote host identity, and non-zero port.  An
      absent configuration leaves the peer listener disabled; malformed or
      wildcard source identity fails before binding. Plain TCP is the supported
      MVP cross-host transport and must work without TLS; TLS is only optional
      future hardening, not a functional precondition.
- [ ] Bind that adapter to the existing canonical Axum router, with one
      connector-owned provenance configuration.  The peer adapter's only
      semantic difference is authentication/provenance normalization before
      the existing router; persistence and hook execution stay shared.
- [ ] Route host-qualified CLI and graft writes through the selected direct
      peer client while preserving local UDS/loopback selection for unqualified
      writes.  The shared peer client must stamp the existing origin metadata
      once, preserve it on the one request, and reject malformed authority or
      port configuration before any connect.
- [ ] Add isolated two-runtime tests covering direct send, source provenance,
      new-write-only hook behavior, same-ID duplicate suppression, invalid
      configuration, and exactly-one direct connection failure.

The outbound `direct_peer_tcp_client` foundation is complete at `6e6a9ecf`:
it is a bounded Reqwest/Tokio connector behind the existing
`HttpRuntimeClient`; all remaining work above is listener composition,
provenance, caller selection, and proof.

## SR-004 — Cross-host physical smoke harness

- [ ] Rework the cross-host Python smoke runner so it preflights the
      replacement binary/revision on both hosts, invokes only `atm send` /
      `atm read` against the replacement configuration, and captures route,
      storage, and hook evidence without SSHing into an ambient user's data.
- [ ] Add unit tests for command construction, required isolated-host
      acknowledgement, source-revision mismatch, and failure reporting.

## SR-006 — Live-proof execution prerequisites

- [ ] Update the physical-proof matrix and benchmark gate with the exact
      commands, required clean OS-user/backup authority, expected evidence,
      operator-owned rollback/park behavior, and separate macOS, M5, and
      Windows rows.
- [ ] Run all focused Rust and Python readiness tests, then `just fmt`,
      `just lint`, `just test`, and `git diff --check` at the final SHA.
- [ ] Send the final commit and an AL.9 review request to team-lead and
      quality-mgr.  Do not represent static/integration proof as an authorized
      host activation.

## Non-negotiable acceptance rules

- One typed HTTP encoder/decoder and one canonical Axum route process local
  and peer writes.
- Storage is durable before a received hook; hook error or timeout returns
  success plus warning and never becomes a write failure.
- A duplicate message ID is informational and does not emit another hook.
- No transport switches silently after a failed request, and one direct
  failure creates no retry/replay state.
- A real live smoke is executed only in an isolated environment and never by
  hijacking an ambient daemon.
