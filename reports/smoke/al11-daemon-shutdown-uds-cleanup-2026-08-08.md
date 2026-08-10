# AL11 daemon shutdown UDS cleanup validation

Date: 2026-08-08

## Finding

`DAEMON-SHUTDOWN-UDS-LEAK-AL11`: the daemon binary terminated with
`std::process::exit`, which skips normal Rust destructor execution. A
long-lived `UnixSocketPathGuard` relies on `Drop` to unlink the socket inode
that it created.

## Change reviewed

`crates/atm-daemon/src/main.rs` now returns `std::process::ExitCode` from its
Tokio entry point. Success remains 0; validation/config errors remain 64;
`DaemonUnavailable` remains 70; all other errors remain 1. Returning from
`main` preserves normal drop glue before the process terminates.

## Simulated clean-shutdown proof

Executed:

```text
cargo test -p atm-http-runtime unix_socket_uses_the_shared_client_router_and_owner_only_endpoint -- --nocapture
```

Result: PASS (1 test). The test binds a real temporary Unix socket through
`HttpRuntime`, invokes `begin_shutdown().finish().await`, and asserts that the
socket path no longer exists. The path is owned by `UnixSocketPathGuard`, so
the assertion proves its `Drop` cleanup unlinked the socket after graceful
shutdown.

## Required validation

```text
just test
cargo clippy -p atm-daemon -p atm-daemon-bootstrap -- -D warnings
```

Both passed. `just test` completed 436 Python checks plus the Rust suite; its
smoke-fixture output includes intentionally exercised failure cases, while the
runner completed with `OK`.
