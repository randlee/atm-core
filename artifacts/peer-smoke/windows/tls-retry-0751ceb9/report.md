# Windows TLS Retry Preparation

## Root Cause

The Windows HTTPS listener builds `PinnedClientVerifier` once at daemon
startup. Although the Mac trust pin was present in SQLite, it had been added
after daemon PID `37996` began, so that listener's in-memory verifier had no
enabled Mac certificate and rejected the mTLS client certificate as `UnknownCA`.

## Remediation And Validation

- Verified durable Mac trust pin: host `10.202.137.160`, fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`, enabled.
- Verified Windows certificate fingerprint:
  `BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`.
- Verified enabled listener record: `10.10.100.98:43101`.
- Stopped PID `37996` before starting the same release binary as PID `39856`.
  There was no overlapping or fallback daemon.
- Listener inspection confirms PID `39856` owns `10.10.100.98:43101`.
- `atm doctor --json` is healthy and ready with one enabled interface and one
  enabled trusted peer.

The Mac operator has been asked to retry the labelled first case. No TLS
bypass, alternate transport, or source change was used. Raw command outputs
remain local and untracked because they include local certificate references.
