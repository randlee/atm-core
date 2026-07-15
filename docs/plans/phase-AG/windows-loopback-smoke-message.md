# Windows-Agent Loopback Smoke Message

Pull `feature/cross-host-communication`, read `docs/plans/phase-AG/windows-loopback-smoke-message.md`, rebuild `atm` + `atm-daemon`, then run the loopback smoke exactly as written and report artifacts/results.

## Purpose

Validate the new daemon-mediated loopback mode on Windows before we rely on it
for cross-host diagnosis.

This smoke is intentionally local-only:

- no macOS peer is required
- no VPN routing is required
- the goal is to prove the Windows daemon can:
  - bind its peer listener
  - accept a peer-transport loopback send
  - persist the delivered message
  - surface it through `atm read`

## Preconditions

- branch: `feature/cross-host-communication`
- worktree is clean enough to build
- no legacy Windows daemon instance is still holding the same ATM runtime state
- repo-local `.atm.toml` contains a daemon listener bind such as:

```toml
[daemon]
peer_listen_addr = "0.0.0.0:43101"
```

## Build

From repo root:

```powershell
cargo build -p agent-team-mail -p atm-daemon
```

## Smoke Setup

Use a clean ATM_HOME and a clean repo-local workspace identity/team for the
smoke. Keep all artifacts under the repo root.

Suggested setup values:

- `ATM_TEAM=windows-loopback`
- `ATM_IDENTITY=windows-agent`

Ensure the roster includes the calling identity before the send:

```powershell
target\\debug\\atm teams add-member windows-loopback windows-agent --home-dir . --json
```

If the member already exists, do not recreate it; use the existing row.

## Required Smoke Steps

1. Start the branch daemon from the repo root.
2. Capture:
   - daemon PID
   - daemon startup transcript
   - `atm doctor --json`
   - listener proof (`Get-NetTCPConnection` or equivalent)
3. Run:

```powershell
target\\debug\\atm send loopback@localhost "windows loopback smoke" --json
```

4. Immediately run:

```powershell
target\\debug\\atm read --json
```

5. Confirm the delivered message body is present in the caller inbox.
6. Repeat with literal loopback IP:

```powershell
target\\debug\\atm send loopback@127.0.0.1 "windows loopback smoke ip" --json
target\\debug\\atm read --json
```

## Required Evidence

Store evidence under:

- `artifacts/phase-AG/windows-loopback/`

Include:

- build transcript
- daemon startup transcript
- `atm doctor --json`
- send JSON for `loopback@localhost`
- read JSON after `loopback@localhost`
- send JSON for `loopback@127.0.0.1`
- read JSON after `loopback@127.0.0.1`
- any daemon log excerpt covering both sends

## Pass Criteria

Pass only if all of the following are true:

- daemon starts from this branch build
- `atm doctor --json` remains healthy/ready
- both loopback sends return success
- both messages are readable from the inbox
- no unexpected daemon crash or restart occurs

## Failure Reporting

If any step fails, stop and report:

- exact failing command
- exact stderr/stdout
- whether the daemon stayed alive
- whether any message was nevertheless persisted
- the retained artifact path
