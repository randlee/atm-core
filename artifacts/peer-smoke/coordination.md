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

- Commit: pending green validation of this branch.
- Preflight: pending.
- Advertised host / certificate fingerprint: pending.

## Windows status (cwin)

- Commit: `a496b1c1` pending merge/push of the Windows report.
- Preflight: release build, loopback CLI send/read/ack, and graft same-host
  smoke passed. Evidence: `artifacts/peer-smoke/windows/preflight-b47c2683/`.
- Persistent release daemon: PID `9284`, loopback listener `127.0.0.1:59081`.
- Advertised host: `10.10.100.98`; it is reserved for the HTTPS peer interface,
  not the local CLI transport.
- Certificate fingerprint: pending local HTTPS interface/certificate setup and
  the Mac operator's matching exchange details. No cross-host send attempted.

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
