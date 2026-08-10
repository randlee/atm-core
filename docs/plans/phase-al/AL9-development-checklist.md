# AL.9 development checklist

Status: active. This checklist turns the AL.9 evidence gate into discrete,
auditable closure steps. It does not authorize legacy-daemon changes, replay,
or public transport-schema changes.

## 1. Freeze the exact subject under test

- [x] Record the AL.8 source SHA, the AL.9 source-and-runtime proof SHA, OS,
      architecture, Rust toolchain, and selected runtime composition in
      `AL9-proof-subject.md`. A release binary identity remains required for
      an authorized physical activation.
- [x] Statically verify the executable selects `atm-http-runtime` composition;
      see `AL9-proof-subject.md`.
- [ ] Prove after the authorized switch that the executable uses
      `atm-http-runtime` composition and that the
      legacy daemon is not an active transport listener.
- [ ] Record team-lead's hard-activation authorization; without that
      authority, keep the run evidence-only.

Why: a proof cannot establish a cutover property if its binary or operator is
ambiguous.

## 2. Close the skipped non-TLS client migration before proving adapters

- [x] Replace CLI `LocalIpcClientTransportAdapter` write dispatch with the
      UDS-preferred/loopback-TCP `DaemonApiClient` connector.
- [x] Replace graft `GraftLocalIpcClientTransport` write dispatch with the
      same shared connector; retain no synchronous write bridge.
- [x] Prove `atm_daemon_client::{exchange_request, try_connect}` and the
      compatibility preflight/dispatch wrapper have no CLI/graft write-path
      caller. Record their retained synchronous read/ack/admin use and async
      conversion/deletion as an explicit AM.1 ledger item; add no new shim,
      TODO, retry, or replay path. Evidence: `AL9-live-reference-graph.md`.
- [x] Add regression coverage proving CLI and graft writes call the same
      `DaemonApiClient::execute(RequestEnvelope::Write)` path and static
      checks that the removed symbol names do not reappear in production.
      `al9_cli_and_graft_send_use_the_selected_runtime_client` locks the
      production send segments to `preferred_local_client` and rejects the
      retained compatibility dispatch there. Shared-client connector tests
      prove the exact encode/decode path and one-attempt direct failure; the
      external graft write remains a physical proof gate in section 4.

Why: the accepted AL.4 asynchronous call shape was incomplete until a physical
connector existed. AL.7 was skipped when TLS left MVP scope, so AL.9 closes
its send-only portion while AM owns the separately scoped async non-write API
conversion and legacy-client deletion.

## 3. Establish the benchmark contract before measuring

- [ ] Locate and preserve the baseline captured at develop `67401907`; do not
      substitute a post-AL dependency graph.
- [ ] Define a fixed request payload/count/concurrency, p50/p99, throughput,
      tolerance, hardware, OS, and toolchain in the result artifact.
- [ ] Measure hook-disabled and hook-active cases, retain raw samples, and
      obtain an actual Windows CI/measurement result.
- [ ] Mark a failed tolerance as a cutover failure: park AL, keep legacy active,
      and do not freeze AM's ledger.

Why: raw comparable artifacts prevent an apparent performance pass caused by a
changed workload or host.

## 4. Execute the local physical-proof matrix

- [x] In-process canonical-router write: record the route, `ApiRouter`
      dispatch, storage boundary, and one post-persist received-hook call.
- [x] Unix UDS write (where supported): prove it uses the shared typed client
      and the same canonical handler/response schema.
- [x] Loopback TCP write: prove authenticated endpoint-record use, the shared
      typed client, canonical handler, storage boundary, and received hook.
- [ ] `atm-graft` outbound write: prove it reaches the shared client and the
      same canonical handler rather than a legacy client path.
- [x] For each direct failure, demonstrate no retry or replay work is created.
      Evidence is the one-exchange client test in `AL9-physical-proof-matrix.md`.

Why: adapter coverage is meaningful only when the proof captures the full
common path, not merely a successful socket connection.

## 5. Obtain externally owned matrix evidence

- [x] Drop the M5 direct-cross-host row from AL.9 execution. It is deferred
      pending a separately assigned secure connector; no plaintext listener or
      TLS-artifact reuse may be introduced to manufacture this proof.
- [ ] Run a Windows physical proof/benchmark result; do not replace it with an
      equivalent-platform claim.

Why: Windows execution cannot be inferred from local macOS tests. M5 is not
an AL.9 gate without an assigned secure connector.

## 6. Publish cutover and ledger-freeze inputs

- [ ] Produce a per-adapter table: add, activation trigger, active owner,
      retire action, rollback owner/action, and endpoint-record publisher.
- [ ] Verify exactly one active listener and exactly one publisher for each
      endpoint during the transition.
- [x] Capture the actual AL.8 live-reference graph, including observability,
      doctor, dashboard, and configuration consumers.
- [x] Hand that graph to AM.1 as the only permitted input for freezing its
      draft deletion ledger; sprint
      numbers must not override the discovered deletion topology.

Why: AM can only delete code once consumers and rollback state are explicit.

## 7. Validate and close

- [x] Run `just test`, `just lint`, format, dependency, and boundary checks at
      the proof SHA. Re-executed at immutable
      `9b4c4799b2d527bcffde228a77cbeff300298138` on 2026-08-10: all five
      commands passed (`just test`, `just lint`, `cargo fmt --all -- --check`,
      `cargo deny check`, and `python3 .just/lint_boundaries.py`).
- [ ] Obtain independent review of matrix, performance artifact, cutover table,
      live-reference graph, and AM ledger input.
- [ ] If any proof, external run, tolerance, or cutover invariant fails, record
      the failure, park AL, leave legacy active, and do not begin AM.

Why: AL.9 is an evidence gate. It is complete only with reviewed evidence, not
with source changes alone.
