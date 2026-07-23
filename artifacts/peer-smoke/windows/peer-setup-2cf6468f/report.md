# Windows Reciprocal HTTPS Setup

- Branch commit before this record: `2cf6468f`.
- Release binaries: `atm` and `atm-daemon` version `1.3.1`.
- One persistent release daemon is running as PID `37996` from this worktree.
- Local HTTPS interface: enabled `10.10.100.98:43101`; Windows listener
  inspection confirms that exact address and port.
- Windows certificate fingerprint:
  `BAF9EC036814C613BBBB77C645DF3AD8A91C5E65D78CF3BDDE900FC7ABB7836F`.
- Mac trust record: enabled host `10.202.137.160`, fingerprint
  `03DC87FA38DD1C20C3528AC9444145C2B1EFA3F98FD46AC0470CCC4BB9730857`.
- `atm doctor --json` is healthy and reports one enabled interface, one
  certificate fingerprint, and one enabled trusted peer.

The certificate/key PEM bundle was generated outside the repository with
owner-only filesystem access. Neither it nor its private-key reference is
included in this evidence.

## Sequencing Finding

The HTTPS listener is constructed only at daemon startup. The existing daemon
was deliberately stopped before the replacement release daemon started, so
there was never more than one daemon. The runbook now explicitly requires this
controlled restart after changing certificate or interface records.

No cross-host message has been sent from Windows. Mac may now add the reciprocal
Windows trust record and begin the labelled Mac-to-Windows case.
