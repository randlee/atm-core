# AB.1 Execution Readiness

## Purpose

This document prepares the next execution session to run AB.1: same-host release-binary
smoke on both hosts under disposable clean-room state. It lists exact commands,
expected evidence to capture, and a pre-session checklist.

AB.2–AB.4 are explicitly out of scope until the listener sprint lands. See
`executability-gap.md`.

---

## Before You Start AB.1: Checklist

- [ ] Verify current IP of each host (DHCP may have shifted):
  - Windows: `ipconfig | findstr IPv4` → confirm `192.168.1.146` or record new IP
  - Mac: `ipconfig getifaddr en0` → confirm `192.168.1.178` or record new IP
- [ ] Confirm Rust `1.94.1` is active on both hosts (`rustup show active-toolchain`)
- [ ] Build release binaries on both hosts (see `windows-host-prereqs.md` and
  `mac-host-prereqs.md` for build commands)
- [ ] Re-verify AB-SMOKE-001 and AB-SMOKE-002 expected outputs against a current build
  (see `ac-freshness-flags.md` — `doctor.rs` and `composition.rs` changed in AC)
- [ ] Confirm live identity directories are not in scope:
  - Windows: `~\.claude\` and `~\.atm\` are not touched
  - Mac: `~/.claude/` and `~/.atm/` are not touched
- [ ] Have `cross-host-smoke-checklist.md` open for logging row results

---

## Windows Host AB.1 Commands

### 1. Set Disposable Environment

```powershell
$env:ATM_HOME        = "C:\Temp\atm-smoke\home"
$env:ATM_CONFIG_HOME = "C:\Temp\atm-smoke\config"
$env:ATM_LOG_DIR     = "C:\Temp\atm-smoke\logs"
$env:ATM_DAEMON_SOCKET = "\\.\pipe\atm-smoke-daemon"

New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\home
New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\config
New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\logs
```

### 2. Start Daemon

```powershell
Start-Process -FilePath ".\target\release\atm-daemon.exe" `
  -RedirectStandardOutput "C:\Temp\atm-smoke\logs\daemon-stdout.log" `
  -RedirectStandardError  "C:\Temp\atm-smoke\logs\daemon-stderr.log" `
  -WindowStyle Hidden
```

Wait 2–3 seconds for the daemon to bind the named pipe before issuing CLI commands.

### 3. Doctor (AB-SMOKE-001)

```powershell
.\target\release\atm.exe doctor --json | Tee-Object -FilePath "C:\Temp\atm-smoke\logs\doctor.json"
```

Expected evidence: well-formed JSON with no error fields. Exact field names TBD in
execution session after AC freshness re-verification.

### 4. List (AB-SMOKE-002, step 1)

```powershell
.\target\release\atm.exe list | Tee-Object -FilePath "C:\Temp\atm-smoke\logs\list.txt"
```

Expected evidence: empty or initialized mailbox listing with no errors.

### 5. Clear (AB-SMOKE-002, step 2)

```powershell
.\target\release\atm.exe clear | Tee-Object -FilePath "C:\Temp\atm-smoke\logs\clear.txt"
```

Expected evidence: success confirmation, no errors.

### 6. Send (AB-SMOKE-002, step 3)

```powershell
.\target\release\atm.exe send --to smoke-self --subject "ab1-win-test" --body "windows same-host smoke" `
  | Tee-Object -FilePath "C:\Temp\atm-smoke\logs\send.txt"
```

Note: `--to smoke-self` or equivalent same-host recipient — verify identity/team
configuration for disposable config root before executing. TBD in execution session.

### 7. Read (AB-SMOKE-002, step 4)

```powershell
.\target\release\atm.exe read --all --json | Tee-Object -FilePath "C:\Temp\atm-smoke\logs\read.json"
```

Expected evidence: JSON array containing the sent message, no errors.

### 8. Capture Daemon Log

```powershell
Copy-Item "C:\Temp\atm-smoke\logs\daemon-stdout.log" "C:\Temp\atm-smoke\logs\daemon-final-stdout.log"
Copy-Item "C:\Temp\atm-smoke\logs\daemon-stderr.log" "C:\Temp\atm-smoke\logs\daemon-final-stderr.log"
```

Retain all log files. Stop the daemon process after evidence is captured.

---

## Mac Host AB.1 Commands

### 1. Set Disposable Environment

```bash
export ATM_HOME="$TMPDIR/atm-smoke/home"
export ATM_CONFIG_HOME="$TMPDIR/atm-smoke/config"
export ATM_LOG_DIR="$TMPDIR/atm-smoke/logs"
export ATM_DAEMON_SOCKET="$TMPDIR/atm-smoke/daemon.sock"

mkdir -p "$ATM_HOME" "$ATM_CONFIG_HOME" "$ATM_LOG_DIR"
```

### 2. Start Daemon

```bash
./target/release/atm-daemon \
  > "$ATM_LOG_DIR/daemon-stdout.log" \
  2> "$ATM_LOG_DIR/daemon-stderr.log" &
DAEMON_PID=$!
```

Wait 2–3 seconds for the daemon to create the Unix socket before issuing CLI commands.

### 3. Doctor (AB-SMOKE-001)

```bash
./target/release/atm doctor --json | tee "$ATM_LOG_DIR/doctor.json"
```

Expected evidence: well-formed JSON with no error fields. Exact field names TBD in
execution session after AC freshness re-verification.

### 4. List (AB-SMOKE-002, step 1)

```bash
./target/release/atm list | tee "$ATM_LOG_DIR/list.txt"
```

Expected evidence: empty or initialized mailbox listing with no errors.

### 5. Clear (AB-SMOKE-002, step 2)

```bash
./target/release/atm clear | tee "$ATM_LOG_DIR/clear.txt"
```

Expected evidence: success confirmation, no errors.

### 6. Send (AB-SMOKE-002, step 3)

```bash
./target/release/atm send --to smoke-self --subject "ab1-mac-test" --body "mac same-host smoke" \
  | tee "$ATM_LOG_DIR/send.txt"
```

Note: `--to smoke-self` or equivalent same-host recipient — verify identity/team
configuration for disposable config root before executing. TBD in execution session.

### 7. Read (AB-SMOKE-002, step 4)

```bash
./target/release/atm read --all --json | tee "$ATM_LOG_DIR/read.json"
```

Expected evidence: JSON array containing the sent message, no errors.

### 8. Capture Daemon Log

```bash
kill $DAEMON_PID
cp "$ATM_LOG_DIR/daemon-stdout.log" "$ATM_LOG_DIR/daemon-final-stdout.log"
cp "$ATM_LOG_DIR/daemon-stderr.log" "$ATM_LOG_DIR/daemon-final-stderr.log"
```

Retain all log files in `$ATM_LOG_DIR` before teardown.

---

## Evidence to Retain

For each host, retain and attach to the AB.1 findings record:

| Artifact | Path |
|---|---|
| Doctor JSON output | `logs/doctor.json` |
| List output | `logs/list.txt` |
| Clear output | `logs/clear.txt` |
| Send output | `logs/send.txt` |
| Read JSON output | `logs/read.json` |
| Daemon stdout (final) | `logs/daemon-final-stdout.log` |
| Daemon stderr (final) | `logs/daemon-final-stderr.log` |

Logs should be copied out of the disposable root before teardown.

---

## Pass Criteria for AB.1

AB.1 passes when, on both hosts independently:

1. `atm doctor --json` returns valid JSON with no error conditions.
2. `atm list` returns without error under disposable state.
3. `atm clear` returns without error.
4. `atm send` delivers a message to the same-host mailbox without error.
5. `atm read --all --json` returns a JSON array containing the sent message.
6. Daemon logs show no panics or unexpected error entries.

---

## Explicit Non-Goal

AB.2, AB.3, AB.4, and AB.5 are out of scope for the AB.1 execution session.
Cross-host rows AB-SMOKE-003 through AB-SMOKE-010 cannot be executed until a
`PeerServerTransport` implementation is merged to develop (suggested sprint label:
`AB.0-peer-server-transport`). See `executability-gap.md` for the full gap analysis
and remediation recommendation.
