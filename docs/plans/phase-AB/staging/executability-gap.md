# Executability Gap: AB.2–AB.4 Cannot Execute on Current Develop

## Headline Finding

AB.2, AB.3, and AB.4 cannot execute on current develop. The cross-host TCP receiver is
not implemented in production code. Any attempt to run cross-host smoke rows
AB-SMOKE-003 through AB-SMOKE-009 will fail at the transport layer regardless of network
configuration or host setup.

---

## Root Cause

The `atm-daemon` has a sender-side TCP peer transport
(`PeerClientTransport::send_to_endpoint`, `peer_transport.rs` lines 341–411). It does
not have a receiver-side TCP peer transport. There is no `PeerServerTransport` struct,
no `TcpListener::bind` in production code (all such calls are inside `#[cfg(test)]`
at lines 934–1309 of `peer_transport.rs`), and no server field in
`PeerTransportRuntime` (line 750).

When the sender's daemon opens a TCP connection to the receiver host, the receiving
daemon has no listener. The connection fails.

For full technical detail including line numbers and architecture spec citations, see
`transport-findings.md`.

---

## Misleading Sprint-Doc YAML vs. Readiness Truth

`sprint-AB2.md`, `sprint-AB3.md`, and `sprint-AB4.md` each carry `status: complete` in
their YAML front-matter. This is a plan-doc artifact — the status field records the
planning and documentation work as complete, not the implementation or execution.

`readiness.md` records every AB sprint as `PENDING` with verdict `NOT READY / BLOCKED`.
This is the accurate operational truth.

There are no remote branches for AB.2, AB.3, or AB.4 work (`git branch -r` shows no
`feature/pAB-s2-*`, `feature/pAB-s3-*`, or `feature/pAB-s4-*` branches). No PR and
no merged commit exists for these sprints. The implementation has not started.

**Recommendation**: The architect should reconcile the sprint-doc YAML `status` fields
with reality. The `status: complete` designation in sprint-AB2/3/4.md is misleading to
any agent or human reading the plan docs in isolation. Options include using a separate
`plan_status: complete` key distinct from an `execution_status` key, or annotating the
existing field with a comment.

---

## Scope of the Listener Implementation

The receiver-side implementation is medium-to-large in effort. It requires:

1. **New `PeerServerTransport` struct** — accept loop using `TcpListener`, per-connection
   task spawn, frame parsing, message dispatch to the local mailbox, acknowledgement
   frame response. Estimated ~150–200 lines of net-new production code.

2. **`ATM_DAEMON_PEER_LISTEN_ADDR` environment variable** — the receiver daemon needs a
   configurable bind address (defaulting to `0.0.0.0:<port>` per architecture §21.6.4).

3. **Config key** — a `daemon.peer_listen_addr` (or equivalent) key in the `.atm.toml`
   schema and config parsing code.

4. **Daemon composition wiring** — `build_peer_transport_runtime` in `composition.rs`
   (line 648) must construct and start the server alongside the client.

5. **Dispatcher routing** — inbound peer messages must route through the same dispatch
   path as local IPC messages, reaching the correct mailbox.

6. **Doctor health surface** — the listener's bind status should be visible in
   `atm doctor --json` output so operators can confirm the listener is up.

7. **Architecture §21.6.4 lifecycle constraints** — the implementation must satisfy:
   - Singleton-safe (no double-bind on daemon restart)
   - Degraded-status reporting on address loss
   - Maximum 64 concurrent accepts
   - Per-connection 5-second connect and 5-second I/O deadlines
   - 30-second retry budget on the sender side (already partially implemented)

---

## Recommended Remediation

Open a fresh sprint before AB.2 begins. Suggested label: **`AB.0-peer-server-transport`**.

This sprint owns the receiver-side listener implementation enumerated above. It should
be opened by the architect, assigned to a dev agent, and QA-gated before AB.2 is
attempted.

AB.2 must not begin until a merged commit on `develop` (or the phase integration branch)
contains a passing `PeerServerTransport` with the §21.6.4 lifecycle constraints
satisfied.

---

## What AB.1 Delivers Independently

AB.1 is not blocked. It exercises same-host release-binary commands under disposable
clean-room state on both hosts individually:

- `atm doctor --json`
- `atm list`
- `atm clear`
- `atm send`
- `atm read --all --json`

This validates that:

- Release binaries build correctly on both platforms.
- Disposable environment isolation (`ATM_HOME`, `ATM_CONFIG_HOME`, `ATM_LOG_DIR`,
  `ATM_DAEMON_SOCKET`) works as specified.
- Local IPC (Unix socket on Mac, named pipe on Windows) is operational.
- Both hosts are ready for cross-host lanes once the listener sprint lands.

AB.1 is a meaningful and complete release-readiness gate in its own right. Its
deliverables (frozen checklist rows AB-SMOKE-001 and AB-SMOKE-002, per-host clean-room
bring-up evidence) are prerequisites for every subsequent lane regardless of when the
listener sprint completes.

---

## Sprint Scope Assignments (for reference when the listener sprint lands)

- **AB.2** owns AB-SMOKE-003 (Win→macOS one-way send) and AB-SMOKE-004 (macOS→Win
  one-way send).
- **AB.3** owns AB-SMOKE-005, 006, 007 (receiver reads and cross-host ack round-trip).
- **AB.4** owns AB-SMOKE-008 (degraded notification after durable send) and
  AB-SMOKE-009 (retry-visible interruption/recovery).
- **AB.5** owns AB-SMOKE-010 (copied-state Lane B revalidation), gated on Lane A pass.
