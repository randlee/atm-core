# AL.8 composition inventory

Captured from the AL.6 parent at `93bb2faf` before the AL.8 composition
cutover. This is an observed source inventory, not AM's deletion ledger.

## Active executable before AL.8

`crates/atm-daemon/src/main.rs` calls
`atm_daemon::run_daemon_with_observability`, which enters
`crates/atm-daemon/src/composition.rs::RuntimeComposition::start`.

The resulting active call graph is legacy and must not remain selected:

```text
atm-daemon main
  -> atm-daemon RuntimeComposition
      -> LocalIpcServerTransportAdapter
          -> legacy local IPC/TCP HTTP workers and raw framing
      -> DaemonRequestDispatcher
          -> legacy runtime-health/post-write execution
      -> legacy 2 s graceful + 3 s force-cancel lifecycle hooks
```

The owner gate is implemented in
`crates/atm-daemon/src/host_ownership.rs`; its `HostOwnershipAdapter::acquire`
must be retained or transplanted before the replacement binds an adapter or
publishes the loopback endpoint record.

## Replacement graph to activate

```text
atm-daemon replacement entrypoint
  -> owner gate
  -> backend-neutral RuntimeAssembly from the approved bootstrap boundary
  -> injected MessageReceivedHookSelector
  -> StorageAndNudgeRouter
  -> HttpRuntime<Configured>::start
      -> Unix UDS (where supported) and/or loopback TCP
      -> canonical_api_router (all retained typed HTTP routes)
      -> one typed API handler (write remains the sole post-persist hook path)
      -> sealed storage/runtime + received-hook boundaries
```

`atm-http-runtime` already owns the canonical Axum route, UDS listener,
loopback listener, capability endpoint record, typed client operation, bounded
write admission, and post-persistence warning semantics. It has no concrete
SQLite/Rusqlite, tmux, graft, bootstrap, raw-frame, peer-only, or replay
dependency.

## Composition gaps opened by this inventory

1. The `atm-daemon` executable still selects legacy composition and must be
   reduced to replacement startup/shutdown only.
2. A replacement composition-owned `MessageReceivedHookSelector` is required.
   It must be injected without adding a concrete tmux or graft dependency to
   `atm-http-runtime`; graft remains independently owned.
3. The runtime lifecycle needs the owner gate and existing readiness/status
   publication linked to its consuming Tokio lifecycle, with the architecture
   5 s drain bound replacing the legacy 2 s/3 s pair.
4. AL.8's static guards must distinguish the unreferenced historical source
   (left for AM deletion) from active executable dependencies, while rejecting
   any active legacy listener/worker/dispatcher edge.
5. The replacement needs route-by-route proof that every retained core HTTP
   operation preserves its response schema and daemon-owned filesystem-root
   policy. The `canonical_api_router` now registers the core route table; the
   active-composition and route-contract tests remain required before this
   graph can be treated as live.
