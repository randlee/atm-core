# Transport Implementation Findings

## Summary

Cross-host ATM messaging requires two production daemons: one on the sending host and one
on the receiving host. The sender side of the TCP transport is implemented. The receiver
side is not. Operational cross-host messaging is therefore not possible on current develop
regardless of network configuration.

---

## Local IPC (same-host): Production, Working

Local messaging uses Unix domain sockets on macOS and Windows named pipes on Windows.
This path is production-complete and was hardened by PR #387
(`feature/windows-test-parity`), which added:

- `apply_local_ipc_deadline()` in `crates/atm-daemon-client/src/lib.rs` — swallows
  `io::ErrorKind::Unsupported` on Windows because named pipes do not support
  `set_send_timeout` / `set_recv_timeout`.
- Extended `#[cfg(windows)]` lock-contention detection in
  `crates/atm-daemon/src/host_ownership.rs`.
- Named-pipe semantics in
  `crates/atm-daemon/src/local_ipc_transport/request_worker.rs` and
  `local_ipc_wake.rs`.

The same-host rows AB-SMOKE-001 and AB-SMOKE-002 exercise this path exclusively.

---

## Cross-Host Transport: Plain TCP, Partially Implemented

### Sender Side — Implemented

File: `crates/atm-daemon/src/peer_transport.rs`

The sender implementation lives in `PeerClientTransport::send_to_endpoint`, lines
341–411. It opens a plain TCP connection to a caller-supplied address, writes the
framed message, and waits for an acknowledgement frame.

Address discovery: the sender reads the environment variable `ATM_DAEMON_PEER_ADDR`
at line 117 to locate the remote daemon's TCP endpoint.

Timeouts: 5-second connect deadline and 5-second I/O deadline (matching the
architecture specification §21.6.4 contract — see below).

Retry budget: `daemon.remote_retry_budget` is the only cross-host key present in the
config schema. No `listen_addr`, `peer_host`, or port key exists anywhere in
`.atm.toml` or the config parsing code.

### Receiver Side — NOT IMPLEMENTED IN PRODUCTION CODE

There is no `PeerServerTransport` struct in the production codebase. There is no
`TcpListener::bind` call outside of the test module.

The test module in `peer_transport.rs` begins at line 934 and runs through
approximately line 1309. All `TcpListener::bind` calls are inside this module and are
gated by `#[cfg(test)]`. They are not compiled into release binaries.

`PeerTransportRuntime` (defined at line 750) has only a `client` field. There is no
server field, no accept-loop task, and no inbound connection handler in the runtime
struct.

### Composition Wiring

File: `crates/atm-daemon/src/composition.rs`

`build_peer_transport_runtime` is defined at line 648. It constructs a
`PeerTransportRuntime` with a client only. The `client` field appears at line 143 of
the same file. No server-side transport is wired into the daemon's composition graph.

---

## Architecture Specification vs. Implementation

The architecture document at `docs/architecture.md` defines the full cross-host contract.
The specification is intentionally ahead of implementation. Key sections:

**§21.4** (line 2630): "cross-host: TCP/TLS" — states that cross-host transport uses
TCP with TLS. TLS is unimplemented on either side. Plain TCP only on the sender.

**§21.6.4** (lines approximately 2984–3051) defines the full receiver lifecycle:

- 5-second connect deadline
- 5-second I/O deadline
- 30-second retry budget
- Wildcard-bind default (0.0.0.0)
- Maximum 64 concurrent accepts
- Degraded-status reporting on address loss

None of this lifecycle is present in production code. The spec defines the contract
that the receiver implementation must satisfy.

---

## Absence of Shared-Filesystem Assumption

No shared-filesystem mechanism (SMB, SSHFS, Syncthing, or similar) is assumed,
referenced, or used anywhere in the codebase or documentation. The only supported
cross-host transport path is TCP daemon-to-daemon. This is consistent with the
architecture specification.

---

## Operational Consequence

For cross-host messaging to work:

1. Both hosts must run `atm-daemon`.
2. The sender's daemon sets `ATM_DAEMON_PEER_ADDR` to the receiver's TCP address.
3. The receiver's daemon must bind a `TcpListener` on a known address and port, accept
   incoming connections, parse framed messages, dispatch them to the local mailbox, and
   respond with acknowledgement frames.

Step 3 does not exist in production code. The connection initiated by step 2 will fail
immediately (connection refused) or time out after the 5-second connect deadline.

This gap is the root cause blocking AB.2–AB.4 execution. See `executability-gap.md`
for the full analysis and recommended remediation path.
