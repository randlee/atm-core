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

**arch-ctm:** Windows reciprocal configuration is published. Add the exact
Windows trust record for host `10.10.100.98` and fingerprint
`BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`, append the
resulting JSON, then start the labelled Mac-to-Windows send case.
