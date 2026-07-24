# Mac↔Windows peer-smoke coordination

This file is the sole coordination channel for the physical smoke. Each
operator pulls before acting, appends its result, commits/pushes immediately,
and polls the branch at least every 60 seconds. Do not route results through a
human intermediary.

## Invariants

- Both hosts use the same commit and matching CLI/daemon release.
- Each host runs exactly one daemon.
- Local loopback and advertised-local-IP proof complete before peer setup.
- Exchange only advertised host and certificate fingerprint; never commit PEM
  private-key material.
- Every accepted peer message retains its original ULID on both hosts.

## Mac status

- Commit: pending push of the candidate startup diagnostic and this record.
- Candidate daemon: one launchd-owned daemon, PID `61975`; `doctor --json`
  reports `running` / `ready` with matching 1.3.1 client and daemon versions.
- Local CLI proof: sent `01KY88848R0AR6RCHZ72DR2JWG` (no ack) and
  `01KY8884FYJN3FRB1P5YK691X4` (requires ack) through local HTTP.
- Advertised host: `10.202.137.160:43101`.
- Certificate fingerprint:
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`.
- Root cause repaired locally: the durable interface record still bound the
  departed address `192.168.128.82`; it now binds/advertises
  `10.202.137.160`. No transport fallback or second daemon was used.

## Windows status (cwin)

- Commit: `a496b1c1`, merged into this shared coordination commit.
- Preflight: release build, loopback CLI send/read/ack, and graft same-host
  smoke passed. Evidence: `artifacts/peer-smoke/windows/preflight-b47c2683/`.
- Persistent release daemon: PID `9284`, loopback listener `127.0.0.1:59081`.
- Advertised host: `10.10.100.98`; it is reserved for the HTTPS peer interface,
  not the local CLI transport.
- HTTPS interface: `10.10.100.98:43101`, enabled and confirmed listening after
  a controlled singleton restart; current release daemon PID `37996`.
- Certificate fingerprint:
  `BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`.
- Trust: enabled durable pin for Mac `10.202.137.160` with fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`.
- Validation: `doctor --json` remains healthy/ready with one enabled interface
  and one enabled trusted peer. Sanitized evidence:
  `artifacts/peer-smoke/windows/peer-setup-2cf6468f/report.md`.
- No cross-host send attempted.

## Exchange and execution order

1. Windows appends passing local preflight facts.
2. Mac appends passing local preflight facts.
3. Each operator adds one exact durable peer-trust record for the other
   host/fingerprint and records the resulting JSON.
4. Mac posts a uniquely labelled cross-host send case; Windows reads/polls it
   and appends the received ULID/result.
5. Windows sends the reciprocal case; Mac records receipt.
6. Execute acknowledgement, duplicate-ULID, unavailable-peer, certificate,
   allowlist, and failed-ack cases. Append each outcome before starting the
   next case.

## Current direct action

**cwin:** the Mac LAN address changed. Update the enabled durable Mac trust
record to host `192.168.128.82` with the unchanged fingerprint
`03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`, disable
the old `10.202.137.160` record, and perform one controlled singleton restart.
Then poll `windows-smoke@atm-dev` for Mac message `01KY8GPS700NA7RQG1JVXX6XQG`,
record the received envelope/ULID, and send one new labelled reciprocal message
to `arch-ctm@atm-dev.192.168.128.82`. Append the result here and push. Do not
start acknowledgement or negative cases yet.

## Mac-to-Windows case 01: TLS diagnostic

- Mac reciprocal trust record is now enabled for Windows `10.10.100.98` with
  fingerprint `BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`.
  Mac `doctor --json` remains healthy with one enabled interface and one
  enabled trusted peer.
- First labelled write: `01KY895Z9PP0V9VY209PRM9ZCT` to
  `windows-smoke@atm-dev.10.10.100.98` reached TLS but failed before HTTP or
  remote persistence: `received fatal alert: UnknownCA`.
- The Mac configured fingerprint is verified against its live PEM bundle as
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`.

**cwin direct action:** root-cause the Windows listener's rejection. Verify
the exact persistent daemon's durable store contains that exact enabled Mac
trust pin, verify the Windows PEM fingerprint is the certificate loaded by the
listener, then perform one controlled singleton restart after the verified
records are in place. Capture the resulting `peer trust list`, `peer
certificate show`, doctor, and sanitized daemon log window. If code/config
defect exists, fix it on this branch, run `just lint && just test`, commit/push
the evidence and result here. Do not add a TLS bypass or alternate transport.

## Windows TLS Diagnostic Result

- Root cause: `HttpsListenerSet` snapshots enabled `TrustedPeer` records at
  daemon startup. The Mac pin was added to durable storage after Windows PID
  `37996` had already built its `PinnedClientVerifier`, so the live verifier
  had no Mac pin and returned `UnknownCA`.
- Verified durable inputs before remediation: enabled Mac host
  `10.202.137.160` with fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`;
  enabled Windows interface `10.10.100.98:43101`; Windows certificate
  fingerprint `BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`.
- Remediation: controlled singleton restart, replacing PID `37996` with
  release daemon PID `39856`. Listener and `doctor --json` are healthy/ready
  with one enabled interface, certificate, and trusted peer.
- Mac was asked to retry case 01. Sanitized evidence:
  `artifacts/peer-smoke/windows/tls-retry-0751ceb9/report.md`.

## Mac TLS restart and case 02

- The Mac trust record for Windows was likewise added after its listener had
  started. A controlled singleton restart replaced the prior candidate daemon;
  current PID `42711` is the only smoke-worktree daemon process.
- `atm doctor --json` is healthy/ready at client and daemon release `1.3.1`,
  with one enabled HTTPS interface and one enabled trusted peer.
- Post-restart labelled Mac-to-Windows write
  `01KY8GPS700NA7RQG1JVXX6XQG` to
  `windows-smoke@atm-dev.10.10.100.98` returned `outcome: sent`.
- The pre-restart Windows retry `01KY89HN323HXVA2JYDX397KAN` was not present
  on the Mac; it is treated as a pre-remediation failed delivery, not a proof
  case.

## Mac current peer endpoint

- Current Mac LAN address is `192.168.128.82`; candidate daemon PID `15899` is
  the sole managed daemon and listens on `192.168.128.82:43101`.
- The enabled durable peer-interface record now binds/advertises that address.
  The certificate fingerprint is unchanged. Windows must refresh its exact
  host-keyed trust record and restart before the reciprocal send.

## Windows Mac Endpoint Refresh And Outbound Finding

- Windows refreshed the durable Mac trust record to enabled host
  `192.168.128.82` with the unchanged fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`, and
  revoked the stale `10.202.137.160` record. The CLI exposes `revoke`, not a
  separate `disable` operation.
- Controlled restart replaced the prior Windows release daemon with PID
  `37876`; `doctor --json` is healthy/ready and listener
  `10.10.100.98:43101` is present.
- The requested reciprocal labelled send to
  `arch-ctm@atm-dev.192.168.128.82` did not return a public CLI result before
  the local request deadline. This is not reliable non-delivery evidence:
  the prior `pong` returned the same CLI `10060` but was received by Mac.
- Root cause: the local CLI/daemon HTTP request deadline is 3 seconds, but
  `HttpsRequestDeadline::default()` permits each synchronous peer network leg
  5 seconds. `DaemonRequestDispatcher::dispatch` ignores the incoming
  `RequestDeadline` when it selects that HTTPS deadline. The client may time
  out and report `failed to read daemon HTTP headers` while peer delivery is
  still in progress or completes. This is a cross-platform product deadline
  contract defect, not a Windows daemon crash or transport fallback issue.
- Do not interpret additional CLI `10060` results as physical peer failures
  until the outer request deadline is propagated to outbound HTTPS work (or
  the public local request deadline is made coherently larger than that work).
  Sanitized Windows evidence:
  `artifacts/peer-smoke/windows/mac-endpoint-refresh-73835a4e/report.md`.

## Consolidated Root-Cause Findings

The five back-to-back Windows sends all returned CLI exit code `4` at
`3029-3051ms`. The daemon log recorded a local `outcome sent` event for each,
but that event is emitted during local persistence before `PostWriteRouter`
performs peer delivery. It does not prove Mac receipt; the earlier conclusion
that all five remote sends succeeded was incorrect.

The complete issue set is:

1. The public local request deadline is 3 seconds, while HTTPS permits 5
   seconds independently for connect, TLS handshake, and request/response.
   A single peer write can therefore outlive the caller by up to roughly 15
   seconds.
2. `ApiRouter::route` receives `RequestDeadline` but only checks it before
   dispatch. `DaemonRequestDispatcher::dispatch` creates a fresh default
   `HttpsRequestDeadline` and does not propagate the remaining outer budget.
3. The daemon does not cancel the in-flight route when the local caller times
   out. The worker can continue peer delivery after the CLI has disconnected,
   leaving remote delivery state indeterminate to the caller.
4. The CLI maps a loopback response-read timeout to `ATM_DAEMON_UNAVAILABLE`,
   which falsely suggests daemon failure. The daemon was healthy throughout.
5. Local worker errors from `handle_connection` are discarded, so failed
   response writes and terminal route errors are absent from daemon logs.
6. The daemon's `outcome sent` observability event is emitted before peer
   delivery completes, making persistence look like end-to-end delivery.
7. Exact-IP peer trust records become stale when a host's advertised address
   changes. The daemon snapshots trust/interface configuration at startup, so
   durable updates require a controlled singleton restart. The runbook also
   referenced nonexistent `trust disable`; the supported operation is
   `trust revoke`.

These are code/contract and operational findings, not evidence of a Windows
daemon crash. The five messages require receiver-side ULID confirmation before
being classified as cross-host successes.

## VPN Target Reconnect Attempt

- Team-lead confirmed the Mac VPN address is `10.212.36.11` on `utun10`.
- Windows replaced the Mac trust target with enabled host `10.212.36.11`,
  revoked `192.168.128.82`, and restarted the singleton release daemon.
- The refreshed daemon is healthy/ready and still listens on
  `10.10.100.98:43101`.
- Direct `curl.exe --connect-timeout 5 --max-time 10 --insecure
  https://10.212.36.11:43101/` timed out at TCP after 5002ms; TLS and HTTP
  were not reached.
- One ATM send to `arch-ctm@atm-dev.10.212.36.11` returned the known local
  3-second response timeout (`10060`). No Windows-side evidence proves remote
  receipt.
- The Mac-to-Windows RDP session is from `10.212.36.11` to Windows
  `10.10.100.98`, proving ingress over the VPN but not Windows egress back to
  the Mac. The remaining blocker is asymmetric VPN routing/firewall policy,
  unless Mac confirms receipt of the ATM message despite the caller timeout.

## Current implementation pass

The smoke branch now adds retained daemon `peer_delivery` events around the
one canonical peer write: `write_persisted`, `attempt`, then `confirmed` or
`unconfirmed`. They distinguish local durable origin persistence from adapter
outcome; they do not yet claim receiver-side evidence. The Windows rebuild and
diagnosis procedure is `artifacts/peer-smoke/windows/rebuild-and-diagnosis.md`.
It requires Cwin to pull, build, run gates, retain one daemon, enable the
10-minute peer-sync window, and root-cause/fix any bounded defect directly on
this branch.
