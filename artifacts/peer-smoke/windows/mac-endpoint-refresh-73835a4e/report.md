# Windows Mac Endpoint Refresh

## Completed Configuration

- Replaced the host-keyed Mac trust record with enabled host
  `192.168.128.82` and fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`.
- Revoked the stale `10.202.137.160` record. The documented `disable` verb is
  not implemented by the CLI; `peer trust revoke` is the available durable
  removal operation.
- Controlled restart left exactly one release daemon, PID `37876`, healthy and
  listening on `10.10.100.98:43101`.

## Outbound Deadline Finding

The reciprocal CLI send did not return before the local request deadline. This
does not prove non-delivery: Mac previously received a `pong` whose local CLI
invocation failed with the same `10060` error.

The code confirms the mismatch:

- `crates/atm/src/composition.rs` sets the public local request deadline to 3
  seconds.
- `crates/atm-daemon/src/local_tcp_transport.rs` gives the router the same
  3-second `RequestDeadline`.
- `crates/atm-daemon/src/runtime_health.rs` ignores that router deadline and
  calls the outbound peer transport with `HttpsRequestDeadline::default()`.
- `crates/atm-daemon/src/https_transport.rs` sets each HTTPS connect,
  handshake, and request leg to 5 seconds.

The local caller can time out while the daemon continues a synchronous peer
write, producing a false-looking daemon-unavailable result. This is a
cross-platform deadline-contract defect. No Windows-specific change, fallback,
or daemon crash was involved.
