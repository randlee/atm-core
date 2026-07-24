# Windows rebuild and peer-diagnosis procedure

Use `origin/evidence/phase-ai-crosshost-smoke` as the only source branch for
this smoke. Pull before each action and append results to
`artifacts/peer-smoke/coordination.md`; do not route results through a human.

## 1. Update and validate

```powershell
git fetch origin evidence/phase-ai-crosshost-smoke
git switch evidence/phase-ai-crosshost-smoke
git pull --ff-only origin evidence/phase-ai-crosshost-smoke
just lint
just test
cargo build --release --bin atm --bin atm-daemon
```

`atm-graft` is a library crate; do not request a nonexistent release binary.

## 2. Controlled singleton replacement

Stop only the known Windows ATM daemon service/process, verify its recorded
PID has exited and its listener is gone, then start the release
`target\\release\\atm-daemon` from this checkout. Do not start a second daemon,
use a fallback transport, or leave mixed CLI/daemon binaries selected.

Verify matching release versions, one daemon, and readiness:

```powershell
atm doctor --json
Get-NetTCPConnection -State Listen -LocalPort 43101
Get-Process atm-daemon
```

Run local CLI and graft loopback send/read/ack proof before peer traffic.

## 3. Reconciliation and observation

Enable the smoke window for the configured Mac peer:

```powershell
atm peer sync-policy set <mac-peer> --max-message-age 600s
atm peer sync-policy show <mac-peer>
```

For every cross-host case, retain the sender ULID and inspect the daemon log
for this sequence:

```text
write_persisted -> attempt -> confirmed | unconfirmed
```

`write_persisted` alone is not a pass. A successful case requires the Mac to
record the same ULID in its mailbox. An `unconfirmed` result is not a daemon
crash; capture the typed error, `atm doctor --json`, and sanitized log window.

Poll `windows-smoke@atm-dev` at least once per minute because Windows has no
nudge dependency in this proof.

## 4. Root-cause and repair rule

If any command, peer delivery, or evidence assertion fails, root-cause it on
this branch. Fix any bounded, testable defect, add/adjust regression tests,
run `just lint` and `just test`, commit/push, and append the commit, exact
failure, root cause, and sanitized evidence path to `coordination.md`.

Do not add a TLS bypass, an alternate transport, a retry queue, a second write
path, a durable IP alias, or a second daemon. If the issue needs an
architecture decision, record the evidence and stop that case only.
