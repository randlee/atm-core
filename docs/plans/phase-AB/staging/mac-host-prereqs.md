# Mac Host Prerequisites

## Host Identity

- Hostname: `Erik_RVS_MacBookPro`
- IP: `192.168.1.178` (DHCP — verify before each execution session; may shift between
  sessions)
- Role: primary orchestrating session and smoke participant (AB.1 same-host,
  AB.2–AB.4 cross-host sender/receiver once listener sprint lands)

---

## AB.1 Scope (Same-Host Clean-Room)

AB.1 exercises same-host commands only. No cross-host connectivity is required.
Prerequisites below cover AB.1 fully. Forward-looking items for cross-host lanes are
called out explicitly.

---

## Rust Toolchain

- Required: Rust `1.94.1` (workspace `rust-version` pin)
- Verify: `rustup show active-toolchain` should report `1.94.1-aarch64-apple-darwin`
  or `1.94.1-x86_64-apple-darwin` depending on hardware
- Install if missing: `rustup toolchain install 1.94.1`
- Set as default for session if needed: `rustup override set 1.94.1`

---

## Build

Build release binaries from the repo root (worktree or main repo, post-AC base):

```bash
cargo build --release -p agent-team-mail -p atm-daemon
```

Expected output artifacts:
- `target/release/atm`
- `target/release/atm-daemon`

Add `target/release/` to `PATH` for the smoke session, or use full paths in commands.

---

## Disposable Environment Variables

All smoke commands must run under disposable roots. Do NOT use or modify live
`~/.claude`, `~/.atm`, or any production identity directories.

Set the following before any smoke command (bash or zsh):

```bash
export ATM_HOME="$TMPDIR/atm-smoke/home"
export ATM_CONFIG_HOME="$TMPDIR/atm-smoke/config"
export ATM_LOG_DIR="$TMPDIR/atm-smoke/logs"
export ATM_DAEMON_SOCKET="$TMPDIR/atm-smoke/daemon.sock"
```

`$TMPDIR` on macOS is a session-scoped temporary directory (typically
`/var/folders/.../T/`). It is cleaned by the OS on reboot. Using it ensures the
disposable state does not accumulate in a fixed location.

Create the directories before first use:

```bash
mkdir -p "$ATM_HOME" "$ATM_CONFIG_HOME" "$ATM_LOG_DIR"
```

Clean-room teardown between runs:

```bash
rm -rf "$TMPDIR/atm-smoke"
```

The `ATM_DAEMON_SOCKET` path uses the Unix domain socket convention. On macOS the
daemon accepts this path and creates the socket file at daemon startup.

---

## Shell Conventions

Use bash or zsh (both are available on macOS). Environment variables are set with
`export VAR=value`. All path handling uses forward-slash separators.

When capturing command output for evidence, use `tee` to write to a log file while
also displaying on the terminal:

```bash
atm doctor --json | tee "$ATM_LOG_DIR/doctor-output.json"
```

---

## macOS Firewall and Local Network Privacy

For AB.1 (same-host), no firewall or privacy changes are needed.

For cross-host lanes (AB.2+, deferred until listener sprint lands):

**macOS Firewall**: System Settings → Network → Firewall. If the firewall is active,
`atm-daemon` will need an inbound rule when it first binds a TCP listener. macOS will
prompt automatically on first bind for a signed binary. For an unsigned local build,
add the rule manually in System Settings → Network → Firewall → Options → `+`.

**Local Network privacy prompt**: macOS may display a privacy prompt the first time an
application makes a LAN connection or multicast query. This prompt appears at the OS
level and must be approved before the connection can proceed. The prompt is expected
and is not an error. Approve it during the execution session when it appears.

These prompts are relevant only to cross-host lanes. AB.1 does not trigger them.

---

## IP Verification Before Each Session

The Mac's IP (`192.168.1.178`) is DHCP-assigned and may change between sessions.
Before each execution session, verify the current IP:

```bash
ipconfig getifaddr en0
```

or

```bash
ifconfig | grep "inet " | grep -v 127.0.0.1
```

If the IP has changed, update `ATM_DAEMON_PEER_ADDR` on the Windows host accordingly
before running any cross-host rows.

---

## Identity Isolation Reminder

The smoke session must not modify or read from:
- `~/.claude/` (live Claude agent identity and team directories)
- `~/.atm/` (live ATM mailbox)
- `~/.claude/teams/` (live agent-team API state)

All ATM operations must use the disposable roots set via the environment variables above.
The `.atm.toml` at repo root sets `identity = "team-lead"` and `default_team = "atm-dev"`.
During smoke testing, override identity and team via environment variables or use a
disposable config root that does not reference production identities.
