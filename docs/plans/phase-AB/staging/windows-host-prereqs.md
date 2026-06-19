# Windows Host Prerequisites

## Host Identity

- Hostname: `2023-001`
- IP: `192.168.1.146` (DHCP — verify before each execution session)
- Role: smoke participant (AB.1 same-host, AB.2–AB.4 cross-host receiver/sender once
  listener sprint lands)

---

## AB.1 Scope (Same-Host Clean-Room)

AB.1 exercises same-host commands only. No cross-host connectivity is required.
Prerequisites below cover AB.1 fully. Forward-looking items for cross-host lanes are
called out explicitly.

---

## Rust Toolchain

- Required: Rust `1.94.1` (workspace `rust-version` pin)
- Verify: `rustup show active-toolchain` should report `1.94.1-x86_64-pc-windows-msvc`
  or the appropriate Windows target
- Install if missing: `rustup toolchain install 1.94.1`
- Set as default for session if needed: `rustup override set 1.94.1`

---

## Build

Build release binaries from the repo root (worktree or main repo, post-AC base):

```powershell
cargo build --release -p agent-team-mail -p atm-daemon
```

Expected output artifacts:
- `target/release/atm.exe`
- `target/release/atm-daemon.exe`

Add `target/release/` to `PATH` for the smoke session, or use full paths in commands.

---

## Disposable Environment Variables

All smoke commands must run under disposable roots. Do NOT use or modify live
`~/.claude`, `~/.atm`, or any production identity directories.

Set the following before any smoke command:

```powershell
$env:ATM_HOME       = "C:\Temp\atm-smoke\home"
$env:ATM_CONFIG_HOME = "C:\Temp\atm-smoke\config"
$env:ATM_LOG_DIR    = "C:\Temp\atm-smoke\logs"
$env:ATM_DAEMON_SOCKET = "\\.\pipe\atm-smoke-daemon"
```

Create the directories before first use:

```powershell
New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\home
New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\config
New-Item -ItemType Directory -Force -Path C:\Temp\atm-smoke\logs
```

The `ATM_DAEMON_SOCKET` named-pipe path follows the convention established by
PR #387 (`feature/windows-test-parity`) and documented in
`docs/cross-platform-guidelines.md`. Named pipes on Windows do not support
`set_send_timeout` / `set_recv_timeout`; `apply_local_ipc_deadline()` in
`crates/atm-daemon-client/src/lib.rs` handles this transparently.

Clean-room teardown between runs:

```powershell
Remove-Item -Recurse -Force C:\Temp\atm-smoke
```

---

## Shell Conventions

Use PowerShell (pwsh or Windows PowerShell) for all smoke commands. Environment
variables are set with `$env:VAR = "value"` syntax. Path separators use backslash
in PowerShell contexts, but the ATM daemon accepts forward-slash paths on Windows
as well.

If comparing command outputs with the Mac host's bash sessions, be aware of line-ending
differences in captured output (CRLF on Windows vs. LF on macOS). JSON outputs from
`--json` flags are not affected structurally.

---

## Windows Firewall

For AB.1 (same-host), no firewall changes are needed.

For cross-host lanes (AB.2+, deferred until listener sprint lands): when
`atm-daemon.exe` first binds a TCP listener, Windows Firewall will prompt for
network access. Allow access for private networks. The prompt appears once per binary
on first bind. If the binary is rebuilt between sessions, the prompt may reappear.

If using a domain or managed environment, a manual inbound rule may be needed:
```
Protocol: TCP
Direction: Inbound
Port: TBD in execution session (determined by ATM_DAEMON_PEER_LISTEN_ADDR value)
Profile: Private
```

---

## OpenSSH

OpenSSH client is included with Windows 11 and requires no installation. It is
sufficient for any file-transfer or remote-command needs during the smoke session.

OpenSSH Server is an optional Windows feature. If the execution plan requires SSH
access from the Mac to the Windows host, install OpenSSH Server during the execution
session setup:

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service -Name sshd -StartupType Automatic
```

Do not install OpenSSH Server during staging. Confirm the need with the execution plan
before the session begins.

---

## Identity Isolation Reminder

The smoke session must not modify or read from:
- `~\.claude\` (live Claude agent identity)
- `~\.atm\` (live ATM mailbox)
- Any team directories under `~\.claude\teams\`

All ATM operations must use the disposable roots set via the environment variables above.
