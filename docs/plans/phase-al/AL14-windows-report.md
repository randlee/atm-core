# AL.14 Windows Status Report

Tested branch: `feature/al-14-smoke`

Candidate source: `6820e49f` after merging `origin/integrate/phase-al` at
`38d49d98`.

## Passing Rows

- `atm doctor --json`: healthy.
- `just smoke localhost`: passed 10/10 repetitions, including send/read and
  requires-ack/acknowledgement reply proof.
- `just smoke local-ip`: passed 10/10 repetitions against the advertised local
  address, including send/read and requires-ack/acknowledgement reply proof.
- `just benchmark`: all F1, F2, F4, F8, F16, and F64 TCP profiles passed with
  durable restart verification.
- `just benchmark-report --rebuild`: passed.
- `just lint`: passed 25/25 checks.
- `just test`: exit 0, 478 Python tests, 2 skipped, Rust tests and doctests
  passed.

## M4 Reachability Evidence

The M4-dependent rows remain blocked before HTTP routing:

- Public `atm send` to the configured M4 alias returned `HTTP client could not
  connect to the configured daemon endpoint`.
- Curl using the configured hostname failed with exit 6, unable to resolve the
  host.
- Curl using the operator-supplied direct VPN endpoint failed with exit 28,
  TCP connection timeout, for both HTTPS and plaintext HTTP probes.
- ICMP ping to the operator-supplied VPN endpoint sent 4 packets, received 0,
  and reported 100% loss.

No route, security, IP, or benchmark-threshold workaround was applied. The
remaining M4 send/ack rows were stopped per the sprint plan.

## Windows Runtime Note

`daemon-switch status --doctor` confirmed the selector pair but also showed
that no `atm-daemon` SCM service is installed on this host. The candidate was
run as one user-owned process through explicit selector links and verified by
executable path, listener, and healthy doctor output.
