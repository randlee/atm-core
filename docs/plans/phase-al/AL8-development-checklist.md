# AL.8 development checklist

This checklist is the implementation and closure ledger for AL.8. It is
derived from `sprint-AL8-daemon-composition-proof.md`; items stay open until
their code, tests, boundary proof, and required validation are complete.

## Entry and scope

- [x] Merge the AL.6 pushed parent (`93bb2faf`) into this worktree.
- [x] Record the active-parent state: AL.7 mTLS is deferred because TLS is
  not MVP scope and the repository already retains an isolated TLS crate for a
  future authorized feature. AL.8 must not introduce a new TLS adapter,
  listener, client, or proof requirement.
- [x] Reconcile the AL.8 sprint's AL.7 dependency with that MVP decision in
  the sprint/phase documentation before calling AL.8 complete.

## Active Tokio/Axum daemon composition

- [x] Inventory the current `atm-daemon` executable entrypoint, ownership
  gate, status/readiness surface, adapter selection, storage construction, and
  received-hook construction. Record each legacy-server call edge that must be
  removed from active composition. Evidence: `AL8-composition-inventory.md`.
- [x] Make `atm-http-runtime` the only server started by `atm-daemon`; do not
  wrap, start, probe through, or retain the legacy `crates/atm-daemon` server
  as a fallback.
- [x] Retain or transplant the existing singleton/owner gate before any
  listener bind or endpoint publication.
- [x] Construct only backend-neutral core storage/router implementations and
  inject the accepted `MessageReceivedHookEmitter` from composition. Runtime
  code must not import SQLite/Rusqlite, tmux, or `atm-graft`.
- [x] **AL8-F001 — production async receiver-hook injection:** provide the
  composition-owned `MessageReceivedHookSelector` required by
  `StorageAndNudgeRouter`. It must select the injected receiver implementation
  by the already-planned harness target, use the inherited absolute deadline,
  and preserve post-persistence success-plus-warning behavior. A test-only
  selector or a permanent `None` selector is not sufficient. Neither
  `atm-http-runtime` nor active daemon composition may import tmux or
  `atm-graft` concrete code.
- [x] Select enabled MVP local adapters (Unix UDS where supported and
  loopback TCP) through typed runtime configuration; no platform-specific
  application route, codec, or listener root is permitted.

## Lifecycle and health

- [x] Wire the existing health/readiness surface to the runtime lifecycle.
  `Ready` requires owner gate, AL.1 validation, canonical router construction,
  and every selected listener bind. `NotReady` applies before start, failed
  startup, and drain. `Live` reflects runtime supervision.
- [x] Return a typed startup cause for failed configuration/bind and prove
  endpoint records are not published before `Running`.
- [x] Use the architecture's single 5-second graceful-drain deadline:
  stop accepts, drain tracked requests, cancel remaining work at deadline, then
  transition to `NotReady`. No detached helper may extend it.
- [x] Add deterministic lifecycle tests for ready ordering, failed start,
  drain/cancel behavior, and the retained five-second bound.

## Canonical semantics and static boundaries

- [x] **AL8-F003 — retained route completeness:** the current replacement
  listener exposes only `POST /v1/atm/messages`, while the frozen core HTTP
  route table also contains list, clear, inspect, read, doctor, runtime reload,
  compatibility, and heartbeat routes. Before activating the replacement
  executable, migrate every retained route through the same framework router
  and connector-neutral client codec, or obtain an explicit MVP API-removal
  decision with matching public-schema/CLI changes. A legacy-server fallback
  is prohibited.
- [x] Prove local adapters reach the one AL.2 canonical handler, the sealed
  storage trait, and the post-persist received-hook call site.
- [x] Cover new write, idempotent duplicate (no second hook), and hook failure
  (durable success plus warning) through active daemon composition.
- [x] Add or update architecture guards proving active `atm-daemon` and
  `atm-http-runtime` contain no raw HTTP framing, peer-only ingress/decoder,
  resend/replay, concrete SQLite/Rusqlite, tmux, `atm-graft`, or legacy-server
  composition dependency.
- [x] **AL8-F002 — active-root guard:** make the boundary test inspect the
  executable's selected composition root and its Cargo dependency closure,
  rather than treating unreferenced historical files as active code. It must
  fail if the executable reaches a legacy listener, worker, dispatcher,
  framing module, replay code, or concrete storage backend.
- [x] Capture a source-level live-reference graph for AM.1 that names the
  active executable, runtime, listeners, canonical handler, storage trait,
  received-hook boundary, and clients. The graph is evidence only; it does not
  freeze AM's removal ledger.

## Closure

- [x] Update boundary manifests and human boundary documents only alongside
  implemented composition changes.
- [x] Update sprint/project-plan status only after every checklist item has
  evidence.
- [x] Run `just fmt`, `just lint`, `just test`, dependency/boundary checks,
  public-schema snapshot checks, and the independent checklist/live-reference
  review. Exact closeout evidence:
  - `just fmt` — passed.
  - `just lint` — passed all 25 checks, including dependency, boundary,
    runtime-wait, fixed-sleep, manifest, and public-schema checks.
  - `just test` — passed: 421 Python tests and the full Rust workspace
    (`atm-daemon` excluded because `autolib = false` leaves its legacy source
    reference-only).
  - `cargo test -p atm-http-runtime --lib` — passed 62 tests.
  - `cargo test -p atm-daemon-bootstrap` — passed 5 tests.
  - `cargo test -p atm-architecture --test boundary_enforcement` — passed 48
    tests.
  - `git diff --check` — passed.

## Critical review round 1

- [x] **AL8-CR-001 — composition-boundary ownership:** the active composition
  manifest was initially placed under `boundaries/atm-daemon`, which would
  falsely make the frozen legacy package appear active. Move it to
  `boundaries/atm-daemon-bootstrap`, make all legacy daemon manifests
  reference-only, and test that the bootstrap is the sole active root.
- [x] **AL8-CR-002 — maintainable runtime methods:** split the 127-line
  lifecycle start method and 146-line retained-route dispatcher into focused
  binding/publication and route-operation methods. This preserves the
  consuming lifecycle and one canonical handler while keeping the function
  length guard green.
- [x] **AL8-CR-003 — enforce every newly declared boundary capability:** add
  source-pattern mappings for all AL.8 `io_forbidden` tags and remove stale
  legacy-daemon dependent declarations so boundary validation fails closed.
- [x] **AL8-CR-004 — production signal-driven drain:** the architecture names
  Unix `SIGTERM` as a daemon control signal. The replacement bootstrap now
  awaits `SIGINT` or `SIGTERM` through Tokio and then consumes the same
  five-second `HttpRuntime` drain transition; no signal thread is introduced.
- [x] **AL8-CR-005 — listener-task supervision:** an unexpected exit of the
  one managed Axum task previously left the bootstrap waiting for a signal
  while health could remain `Ready`. The task now has a cancellation-safe
  termination guard, revokes readiness, wakes bootstrap supervision, and
  reaches the normal endpoint cleanup path. A deterministic abort test proves
  the transition and record cleanup.

## Validation finding

- [x] **AL8-GATE-001 — shared notification-log test correlation:** the full
  workspace suite exposed a test that assumed its delivery notification was
  the final record in the host-scoped log. Real concurrent ATM traffic makes
  that assumption false. The test now locates its event by its unique message
  ID, preserving the assertion without treating a shared production log as a
  private fixture.
- [x] **AL8-GATE-002 — stale TLS-boundary expectation:** the active daemon no
  longer consumes the isolated TLS helper boundary, but an architecture test
  still expected it in `allowed_dependents`. The guard now expects only the
  remaining TLS interop consumer while separately retaining the daemon's
  forbidden TLS edge.

## Critical review round 2

- [x] Re-read the AL.8 sprint against the active executable, Cargo graph,
  canonical route table, lifecycle transitions, and boundary manifests after
  all fixes. The executable is a thin Tokio entrypoint; its selected bootstrap
  is the sole allowed concrete storage composition point; and
  `atm-http-runtime` has no SQLite, tmux, graft, raw-frame, peer-only, or
  replay dependency.
- [x] Review the active server-supervision path. **AL8-CR-005** was the only
  gap found and is fixed above. A final re-read after that fix found no further
  AL.8-scope issue. Legacy daemon source still contains old framing and thread
  code, but `autolib = false`, static active-root guards, and the live graph
  prove it is reference-only pending Phase AM deletion.
