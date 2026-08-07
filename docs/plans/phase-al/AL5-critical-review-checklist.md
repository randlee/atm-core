# AL.5 Critical Review Checklist

Review baseline: `efd753d4` (AL.5 refreshed from `integrate/phase-al`).

This checklist is the post-implementation closure review for the Unix UDS
adapter. It keeps the AL.5 adapter limited to Tokio/Axum listener lifecycle and
the AL.4 shared Reqwest client; it must not create a UDS-specific decoder,
router, storage call, retry loop, or legacy-daemon dependency.

## Findings and closure work

- [x] **RSH-001 / AL5-RV-001 — atomic owner-only socket publication.** The
  parent preflight intentionally permits normal non-writable `0755` parents,
  while rejecting group/other writes. Bind in a `0700` staging directory,
  apply and verify the configured socket mode, then rename to publish the
  prepared inode. This avoids a process-global `umask` and its multi-threaded
  side effects.
- [x] **AL5-RV-002 — wire-level response parity.** Existing tests prove typed
  UDS dispatch, but not that a real UDS HTTP response has the same status,
  headers, and JSON bytes as the in-process canonical route. Add that direct
  comparison using one `CanonicalWriteHandler` fixture.
- [x] **AL5-RV-003 — in-flight graceful drain.** Existing UDS shutdown test
  stops an idle listener only. Add an actual UDS request that enters the
  canonical handler, begin shutdown while it is in flight, prove the runtime
  neither completes nor unlinks the socket early, then release the request and
  prove successful drain and cleanup.
- [x] **RBP-F001 — prevent owner/mode parameter swaps.** Replace the two
  same-typed numeric `UnixSocketConfig::new` parameters with semantic
  `UnixSocketOwnerUid` and `UnixSocketMode` newtypes.
- [x] **RBP-F002 — preserve structured I/O causes.** Use `AtmError` recovery
  text plus `.with_cause(source)` for TCP bind/address and runtime join I/O
  failures, consistent with UDS setup errors.
- [x] **RBP-F003 — one socket-path validation rule.** Reuse one runtime-owned
  empty-path validator from both UDS server configuration and the shared UDS
  client factory.
- [x] **RSH-002 — do not block the Tokio worker during filesystem setup.**
  Run the short preflight/staging/bind/permission operation in Tokio's managed
  blocking pool; the request path remains fully async.
- [x] **RSH-003 — drain both additive listeners on first completion.** Replace
  `try_join!` with a coordinator that signals graceful shutdown to the sibling
  then awaits it, preserving the first failure rather than dropping a healthy
  sibling.
- [x] **ARCH-001 — retain the runtime boundary allowlist.** Replace the
  production `tempfile` staging dependency with a std-only owner-checked
  staging directory allocated from process id plus atomic counter, then prove
  the UDS socket is still published only after mode verification.

## Verified non-findings

- Request handling uses `axum::serve`, Tokio `TcpListener`/`UnixListener`, and
  an async Reqwest connector. There is no raw socket framing, polling loop,
  detached request task, second decoder, UDS-only router, or legacy-daemon
  reference in `atm-http-runtime` production transport code.
- The one `tokio::task::spawn_blocking` call is the deliberate bounded SQLite
  seam in `StorageAndNudgeRouter`; it awaits its durable result and is not a
  synchronous router or worker pool. It is required until the storage boundary
  itself becomes asynchronous.
- Synchronous filesystem metadata, permission application, and inode-safe
  cleanup are startup/shutdown ownership operations required by the Unix socket
  API. They are not request-path I/O. AL5-RV-001 narrows their publication
  security invariant.
