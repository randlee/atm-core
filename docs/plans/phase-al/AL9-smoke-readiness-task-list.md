# AL.9 smoke-readiness task list

**Status:** active implementation checklist.  A check is complete only when its
targeted tests pass.  It does not authorize activating or altering an ambient
daemon; live evidence remains an operator-owned AL.9 gate.

This list supersedes AL.9's earlier source-only proof status for the purpose of
preparing smoke execution.  All new work is in the Tokio/Axum
`atm-http-runtime` line; `atm-daemon` remains a thin replacement-runtime
executable and the frozen legacy daemon implementation is out of scope.

## SR-001 — Replacement binary identity and isolated CLI smoke

- [x] Review every local smoke entry point to prove that the child executable
      is the thin `atm-daemon` replacement launcher, whose only serving path
      is `atm_daemon_bootstrap::run_replacement_daemon`.
- [x] Make the CLI smoke harness retain its explicit isolated-user preflight,
      start its own child through `atm doctor`, reject any non-ready runtime
      projection, and then prove `atm send` reaches the canonical Axum route.
      `doctor` is the readiness wait: it cannot return a ready replacement
      projection before the child has published its endpoint and served the
      canonical request.
- [x] Cover success, refusal to attach to an ambient daemon, and wrong-binary
      identity in Python unit tests.  The harness must never kill or switch an
      ambient daemon.

## SR-002 — Localhost and same-IP route-to-hook proof

- [ ] Add an isolated runtime smoke harness that exercises `localhost` and
      the selected loopback address through the public typed client and
      records one durable write and one received-hook result per new message.
- [ ] Include a repeated same-ID case proving no second received hook, and a
      hook failure/timeout case proving durable success plus warning.
- [ ] Ensure the proof reads the same canonical response schema for UDS and
      loopback; no raw/legacy dispatcher is permitted.

## SR-003 — Direct non-TLS cross-host transport

- [ ] Introduce a bounded, explicitly configured plain-TCP peer adapter in
      `atm-http-runtime`.  It must use `HttpRuntimeClient`'s existing request
      encoder, response decoder, deadline, and one-exchange failure behavior;
      it may not add a peer DTO, array grammar, retry, replay, or hand-rolled
      HTTP.
- [ ] Bind that adapter to the existing canonical Axum router, with one
      connector-owned provenance configuration.  The peer adapter's only
      semantic difference is authentication/provenance normalization before
      the existing router; persistence and hook execution stay shared.
- [ ] Route host-qualified CLI and graft writes through the selected direct
      peer client while preserving local UDS/loopback selection for unqualified
      writes.  Define the fixed smoke port/configuration and reject malformed
      configuration before any bind/connect.
- [ ] Add isolated two-runtime tests covering direct send, source provenance,
      new-write-only hook behavior, same-ID duplicate suppression, invalid
      configuration, and exactly-one direct connection failure.

## SR-004 — Cross-host physical smoke harness

- [ ] Rework the cross-host Python smoke runner so it preflights the
      replacement binary/revision on both hosts, invokes only `atm send` /
      `atm read` against the replacement configuration, and captures route,
      storage, and hook evidence without SSHing into an ambient user's data.
- [ ] Add unit tests for command construction, required isolated-host
      acknowledgement, source-revision mismatch, and failure reporting.

## SR-005 — Benchmark hook modes and evidence integrity

- [x] Give replacement bootstrap an explicit test-only hook mode selection:
      `active` uses the injected receiver hook and `disabled` selects no hook.
      The normal production/default mode remains active.  Invalid modes fail
      before listener publication.
- [x] Teach `run_admission_capacity.py` to select and record either mode,
      assert the replacement readiness marker, and label the stage correctly:
      a hook is awaited after commit, while its failure is returned as a
      warning rather than a failed write.
- [x] Add Python tests for argument/environment construction and evidence
      schema; add Rust bootstrap tests for selection and invalid values.

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
