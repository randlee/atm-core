# Windows preflight report

- Tested commit: `b47c268380c8117b6868c9320f3715d9df328ced`
- Client/daemon version: `1.3.1`
- Persistent release daemon: PID `9284`, listening on `127.0.0.1:59081`
- Windows advertised IPv4: `10.10.100.98`

## Validation

- `just lint`: pass after the smoke-branch fixes.
- `just test`: pass after the smoke-branch fixes and before the persistent
  daemon launch. A later rerun correctly failed only because that required
  daemon holds the host-wide owner lock and daemon tests create isolated
  owners; it was left running as required by this smoke procedure.
- `cargo build --release --bin atm --bin atm-daemon`: pass.
- `scripts/smoke/run_graft_same_host.py`: pass. The runner owned and removed
  its temporary smoke daemon; the persistent release daemon was started only
  after that smoke completed.

## Local CLI proof

The release CLI completed the loopback-TCP send/read/ack flow through the
persistent daemon:

- no-ack message: `01KY87GCPR6D448MQ8SRC43CVD`
- requires-ack message: `01KY87GR0KC9BRG5VG206NES0R`
- acknowledgement reply: `01KY87GR4KR174D4EZG0RT0Y6H`

The graft smoke also completed its library-host flow; its emitted ULIDs and
structured result are retained in `graft-same-host-smoke.txt`.

## Fixes applied

- Updated the peer-pair runner test to assert its current `public ATM CLI`
  validation error text.
- Corrected this Windows runbook: public local CLI traffic is loopback-only.
  The advertised LAN address is for the HTTPS peer interface after reciprocal
  trust is exchanged, not a local client endpoint.

## Peer status

- Peer interfaces: none configured.
- Local certificate: none configured.
- Trusted peers: none configured.
- No cross-host send was attempted. Awaiting the Mac operator's advertised
  host and certificate fingerprint before durable peer configuration.
