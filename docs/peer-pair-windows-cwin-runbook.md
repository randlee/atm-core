# Windows preflight for the Mac↔Windows peer smoke

Run this from the Windows checkout at the exact commit supplied by the Mac
operator. Record all command output in the shared evidence directory. Do not
change ATM source, use a fallback transport, or start more than one daemon.

## 1. Match the release candidate

```powershell
git fetch origin
git checkout <tested-commit>
git rev-parse HEAD
just lint
just test
cargo build --release --bin atm --bin atm-daemon --bin atm-graft
.\target\release\atm.exe --version
```

The reported commit and client version must be included in the evidence. Stop
and report a mismatch; do not test different commits on the two hosts.

## 2. Start exactly one Windows daemon

Use the release `atm-daemon.exe` built above and the normal Windows runtime
location. Windows has no UDS or named-pipe local client path: both `atm` and
`atm-graft` must use the daemon's loopback TCP HTTP interface.

Before starting, inspect existing owners/listeners. Stop only an identified
old ATM daemon. Start one persistent release daemon, then capture:

```powershell
Get-Process atm-daemon -ErrorAction SilentlyContinue
Get-NetTCPConnection -State Listen | Where-Object LocalPort -in 43101,43145
.\target\release\atm.exe doctor --json
```

`doctor` must report `liveness: running`, `readiness: ready`, and matching
client/daemon versions. Record the daemon PID and every listener. A named
pipe, UDS, second daemon, or alternate local listener is a stop-and-report
failure.

## 3. Prove local Windows transport before peer work

Use the public release clients only. Run each test first through `127.0.0.1`
and then the Windows machine's advertised IPv4 address. For each address:

1. send a no-ack message to a local test identity;
2. read/peek its exact message ID;
3. send a `--requires-ack` message;
4. read it, run `atm ack <message-id> <reply>`, and read the reply;
5. repeat the send/read/ack proof through `atm-graft`.

Record every command, message ULID, and JSON result. The daemon remains the
same singleton for all local proof. Do not infer that a raw TCP connection is
delivery evidence.

## 4. Provide the Mac operator the peer setup facts

After local proof, publish a sanitized preflight record containing:

- tested commit, client version, daemon version and PID;
- Windows advertised IPv4 address and HTTPS listener bind/advertise record;
- `atm peer interface list --json`, `atm peer certificate show --json`, and
  `atm peer trust list --json` outputs (redact only private-key material);
- listener inspection and the local proof ULIDs/results;
- a sanitized daemon log window.

Poll the shared evidence folder and ATM inbox periodically. Windows has no
nudge support in this first smoke pass; polling is the required notification
mechanism.

## 5. Configure reciprocal peer trust only after exchange

The Mac and Windows operators exchange advertised host and certificate
fingerprint out of band, then each uses only durable CLI-managed records:

```powershell
.\target\release\atm.exe peer interface set --bind <windows-ip:43101> --advertise-host <windows-ip> --enabled
.\target\release\atm.exe peer trust add --host <mac-advertised-host> --fingerprint <mac-fingerprint> --yes
```

Use `peer trust replace` only when replacing an already-recorded peer with the
agreed fingerprint. Never use environment variables, raw sockets, or ad-hoc
address overrides for peer configuration.

## 6. Execute the physical peer smoke

With reciprocal configuration active, run every AI.13 case in both directions:

1. send/read/poll-visible nudge;
2. requires-ack followed by remote ack reply;
3. exact-ULID duplicate (one immutable record, no repeated nudge);
4. unavailable peer (typed error, no prohibited delivery state);
5. bad certificate and non-allowlisted peer (rejected before routing);
6. failed remote ack (source message remains unacknowledged).

The same original ULID must be visible on both hosts for every accepted
cross-host message. A test is passing only after the receiving host reads the
message and the expected acknowledgement is observed; successful socket
connection alone is not evidence.

Run the repository runner after the two host configs are completed:

```powershell
python scripts/smoke/run_peer_pair.py --config peer-smoke-role-b.json --evidence-dir artifacts/peer-smoke/windows
```

Its configuration must invoke public `atm`/`atm-graft` commands and must not
claim ownership of the persistent system daemon. Preserve the sanitized JSON
evidence, doctor reports before/after, listener/PID output, and log window.

## 7. End state

After the suite, re-run `atm doctor --json` and listener inspection. Leave the
one pre-existing persistent daemon running; the runner may stop only a daemon
it launched and explicitly owns. Report failures with the case ID, exact
command/result, both host commits/versions, and sanitized logs.
