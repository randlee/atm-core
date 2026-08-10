# AL.3 Replacement Runtime — Remaining Work

Status: **AL.3 carry-forward closed through AL.5 — verified 2026-08-07**. This is a replacement-runtime
checklist, not a legacy-daemon remediation plan. `crates/atm-daemon` is
reference-only until Phase AM deletes it. No task in this file permits changing,
wrapping, starting, or using its listeners, workers, `ApiRouter`, hook
implementation, or tests as proof.

## Non-negotiable execution path

```text
Tokio/Axum ingress
  -> one typed replacement write service
  -> injected storage/runtime boundary commits the canonical write
  -> new record only: injected MessageReceivedHookEmitter
  -> durable success, optionally with a hook-warning
```

The same absolute `RequestDeadline` flows through the whole request. Storage
failure is an error and persists no record. A hook start, execution, or timeout
failure after a commit is a successful write with a stable warning. An
idempotent duplicate is successful and does not emit a second hook.

## Closure tasks

- [x] **AL3-RH-000 — Replacement-owned Tokio/Axum server.**
  - `atm-http-runtime` binds and serves the canonical typed Axum write route on
    the Tokio runtime supplied by the replacement executable; it never creates
    a private runtime or calls `Handle::block_on`.
  - Its consuming lifecycle owns listener, cancellation, task join, and bounded
    shutdown. It has no dependency on `atm-daemon`, concrete SQLite, tmux,
    graft, or daemon bootstrap.
  - Tests prove binding, a real HTTP request, orderly shutdown, and no leaked
    server task.

- [x] **AL3-RH-001 — Replacement-owned storage-and-hook write service.**
  - Add the one replacement `CanonicalWriteHandler` implementation in
    `atm-http-runtime`. It calls only existing core/runtime composition types:
    injected storage-backed `LocalServiceRuntime`, injected observability, and
    injected `MessageReceivedHookEmitter`.
  - The service prepares and commits the canonical `WriteRequest` through the
    existing core writer, then invokes the received hook only for a newly
    persisted message. It must not call any legacy daemon router or worker.
  - Tests prove storage is called before the hook; database failure runs no
    hook; duplicate delivery runs no second hook; and hook failure becomes an
    existing-schema warning on success.

- [x] **AL3-RH-002 — Tokio isolation and deadline contract.**
  - Synchronous storage and the current synchronous, sealed hook operation run
    only in narrowly scoped `spawn_blocking` work; no Tokio worker is blocked.
    The HTTP task awaits the result and never detaches a write or hook.
  - Pass one absolute deadline unchanged. Reject before admitting expired
    work, retain a started transaction's real durable outcome, and do not add a
    second full-duration timeout.
  - Tests cover zero budget, storage error, successful commit, hook failure,
    exhausted post-commit budget, and duplicate/no-hook.

- [x] **AL3-RH-003 — Replacement proof and legacy quarantine.**
  - Default repository tests and CI exclude `atm-daemon` unit tests immediately;
    they are historical reference tests, not acceptance evidence for AL.
  - While the new executable target is being completed, legacy may still be
    compiled only where workspace metadata requires it; it must not be started,
    smoke-tested, or selected by an AL proof. AL activation replaces the binary
    before AM deletes the legacy crate.
  - Add an architecture guard forbidding `atm-http-runtime` from depending on
    `atm-daemon`, `Runtime::Builder`, `Handle::block_on`,
    `std::sync::mpsc`, `std::thread::sleep`, or legacy transport module names.

- [x] **AL3-RH-004 — Replacement adapter ownership and tracking verified
  (2026-08-10).**
  - AL.5 UDS and AL.6 loopback TCP use this one replacement server and
    service; the current direct plaintext peer route uses the same shared
    client path. They add no peer-only decoder, queue, listener, or post-send
    path. AL.7 peer TLS was removed from MVP scope and was never implemented,
    so there is no TLS adapter to own or revive.
  - The hook boundary's async evolution, process cleanup details, and the
    `sc-lint` blocking-sleep product rule remain tracked in open product issue
    [sc-lint#82](https://github.com/randlee/sc-lint/issues/82). They remain
    replacement-owned work only and never authorize repairing the old daemon.

## 2026-08-06 critical-review findings

The listener and direct async handler have been added. Checked items below
have current AL.4 evidence; unchecked items remain required before this
implementation can be considered the replacement ingress.

### AL.4 carry-forward disposition

AL.3 is merged; its unresolved replacement-runtime findings are therefore
implemented and verified on the AL.4 worktree rather than by rewriting the
merged AL.3 branch. The three correctness-critical items below are complete at
`feature/pal-s4-shared-client-retrigger`:

- `1d72aa93` adds the real Axum-route storage-to-hook outcome matrix.
- `f3746155` makes the injected hook future cancellable and bounded by the
  inherited absolute request deadline.
- `bd7a4513` adds the single-permit Tokio-owned SQLite admission boundary and
  its saturation, cancellation, error, and started-job outcome tests.

The focused replacement test suite is
`cargo test -p atm-http-runtime storage_and_nudge_router`; it exercises these
behaviors through `canonical_message_router`, not private helper calls.

The remaining AL.4/AL.5 carry-forward closure evidence is retained on the
integration line: `83029ac6` (harness selection), `930316dd` (sealed handler),
`b47466f1` (daemon-owned filesystem context), `c7b2d731` (shared post-commit
planning), `d825202c` (legacy quarantine), `74187aa2` (honest staged
configuration), and `9c6c280b` (neutral compatibility oracle). These commits
are ancestors of the AL.6 baseline; this document is a status record, not a
license to re-open legacy daemon code.

- [x] **AL3-CR-001 (blocking) — Prove the actual Storage → hook outcome
  matrix.**
  - `storage_and_nudge_router` constructs `StorageAndNudgeRouter` with a real
    SQLite-backed `LocalServiceRuntime`, recording/failing hook, and
    `canonical_message_router`.
  - The Axum-route tests prove storage failure has neither record nor hook; a
    new durable record gets exactly one hook; duplicate delivery gets no
    second hook; and a hook error is a successful response with the existing
    warning schema.
  - The recording hook loads the committed record while executing, proving
    durable persistence precedes hook emission.

- [x] **AL3-CR-002 (blocking) — Enforce a real post-commit hook deadline.**
  - `AsyncMessageReceivedHookEmitter` returns a cancellable future. The router
    awaits it with `tokio::time::timeout` using only the remaining portion of
    the inherited absolute request deadline.
  - On timeout, durable success is retained with a warning; the timed-out
    future is dropped rather than detached. The route-level stalled-emitter
    test proves both the warning and cancellation cleanup.

- [x] **AL3-CR-003 (blocking) — Use bounded replacement-owned write
  admission, not Tokio's generic blocking queue.**
  - `WriteAdmission` is the single-permit, Tokio-owned admission boundary
    immediately before the narrow SQLite `spawn_blocking` seam. It awaits
    completion for the caller and accepts no detached job.
  - Its tests cover saturation without starting a second job, cancellation
    while queued, storage errors, and preserving a started transaction's real
    durable outcome after the advisory deadline. It contains no legacy worker
    pool, thread queue, retry, or replay path.

- [x] **AL3-CR-004 (important) — Make harness-specific hook selection
  explicit.**
  - `StorageAndNudgeRouter` owns one global
    `Arc<dyn MessageReceivedHookEmitter>`. A tmux emitter rejects a graft
    target and a graft emitter rejects a tmux target, so this cannot satisfy a
    mixed roster.
  - Replacement composition must select the appropriate injected receiver
    implementation from the committed recipient/harness before emission.
    `atm-http-runtime` must remain unaware of tmux and graft concrete types;
    it receives only a core-owned, object-safe selection boundary or a
    harness-resolved emitter.
  - Test both harness selections and a recipient with no hook capability. No
    branch may reintroduce a daemon dependency on `atm-graft`.

- [x] **AL3-CR-005 (important) — Remove the unnecessary public handler
  extension point.**
  - `CanonicalWriteHandler` is a public, unsealed trait even though the
    replacement has one intended production implementation,
    `StorageAndNudgeRouter`. This is an avoidable public implementation
    surface and conflicts with the fixed, simple replacement composition.
  - Either make the handler implementation an internal concrete dependency of
    the route, or make the boundary deliberately sealed and document why an
    external implementation is required. Prefer the former; tests can use
    crate-private test seams without publishing a plugin API.

- [x] **AL3-CR-006 (important) — Stop trusting caller-owned filesystem paths
  in the server path.**
  - The replacement currently passes `WriteRequest.home_dir` into the
    post-commit record lookup and hook-plan construction. That is a
    client-supplied path even though `LocalServiceRuntime` documents that a
    system daemon must not read caller-owned workspace state.
  - Inject a daemon-owned runtime/home context at composition and prove a
    forged request path cannot redirect server filesystem access. Preserve the
    existing wire struct; normalize it at the ingress boundary rather than
    creating a second request schema.

- [x] **AL3-CR-007 (important) — De-duplicate post-commit hook planning in
  core.**
  - `emit_received_message_after_commit` substantially duplicates
    `emit_persisted_local_post_write`'s record-load, recipient-resolution, and
    delivery-plan setup. Two copies will drift and re-create the legacy
    complexity this replacement is meant to remove.
  - Extract one small core helper that builds the receiver-hook dispatch from
    a committed record. The replacement calls the hook-only operation; the
    legacy reference function may retain its separate delivery execution until
    Phase AM deletes it. Add parity tests for the shared plan construction.

- [x] **AL3-CR-008 (important) — Complete legacy test/runtime quarantine.**
  - Default `just test` and the CI workspace unit-test step now exclude
    `atm-daemon`; this part is complete.
  - CI still builds, installs, and smoke-runs `atm-daemon`. Until the
    replacement executable is composed and substituted, those jobs must be
    labelled historical/reference only and must not be used as AL acceptance
    evidence. At activation, replace them with the new executable's smoke;
    Phase AM then removes the old jobs and crate.
  - Add an architecture test that rejects `atm-http-runtime` references to
    legacy daemon modules, `Runtime::Builder`, `Handle::block_on`,
    `std::sync::mpsc`, and production `std::thread::sleep`.

- [x] **AL3-CR-009 (minor) — Keep configuration honest during staged
  adapters.**
  - `HttpRuntime::start` currently starts plaintext TCP only, although its
    validated configuration also contains UDS and TLS material. Make inactive
    adapter configuration unrepresentable for this stage, or explicitly
    document and test that it is preflight-only until its owning adapter
    sprint. Do not imply that validating TLS material enables TLS.

- [x] **AL3-CR-010 (minor) — Move the compatibility oracle out of the legacy
  daemon documentation path.**
  - The new runtime's test reads `docs/atm-daemon/openapi.yaml`. Move the
    canonical OpenAPI/typed write contract to a neutral API location before
    Phase AM deletes legacy-daemon material, then have both client and
    replacement tests consume it.

## Required outcome matrix

| Storage outcome | Hook outcome | HTTP caller result |
|---|---|---|
| Rejected before start / storage error | Not run | Existing machine-readable error; no row |
| New record committed | Success | Success |
| New record committed | Start, execution, or timeout failure | Success + warning |
| Idempotent duplicate | Not run | Success; no hook warning |

## Non-goals

- No sender retry, replay, resend state machine, notification queue, or
  peer-specific ingress grammar.
- No legacy daemon test, listener, worker, or hook execution.
- No direct concrete SQLite, tmux, or graft dependency in `atm-http-runtime`.
