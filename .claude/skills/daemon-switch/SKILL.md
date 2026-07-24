---
name: daemon-switch
description: Safely inspect or switch the system-wide ATM CLI and daemon as one matched release pair. Use before daemon smoke testing, after daemon incompatibility or missing-daemon failures, and to restore the installed release afterward.
---

# Daemon Switch

Use `scripts/daemon-switch.py`; never point a system LaunchAgent/service at a
worktree binary directly.

## Rules

1. Query first: `python3 .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor`.
2. Switch `atm` and `atm-daemon` together, then restart exactly one managed
   service. The script refuses an unpaired or unmanaged switch.
3. Verify native `atm doctor --json` after every switch.
4. After smoke completes or aborts, run `restore` to select the latest installed
   release and repeat the doctor check.
5. If recovery fixes a missing or incompatible daemon, notify the team after it
   is healthy again.

## Commands

macOS requires the existing LaunchAgent label and plist. Linux and Windows use
their existing service name.

```sh
# Inspect selected CLI/daemon binaries and the live daemon version.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor

# Switch both Homebrew links to a branch build, controlled-restart one daemon.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch \
  --cli target/release/atm --daemon target/release/atm-daemon --yes \
  --service com.atm.daemon --launch-agent-plist ~/Library/LaunchAgents/com.atm.daemon.plist

# Restore the latest Homebrew formula targets, not a Cellar path.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py restore --yes \
  --service com.atm.daemon --launch-agent-plist ~/Library/LaunchAgents/com.atm.daemon.plist
```

On systems without Homebrew, provide `--default-cli` and `--default-daemon` to
`restore`. Use `--dry-run` before the first switch on an unfamiliar host.

## Windows selector provisioning

The script never replaces ordinary `.exe` files. Provision two user-writable
selector symlinks named `atm.exe` and `atm-daemon.exe` in one directory placed
before the installed release directory on `PATH`, then pass them explicitly:

```powershell
python .claude/skills/daemon-switch/scripts/daemon-switch.py switch `
  --cli-link C:\\atm-active\\atm.exe --daemon-link C:\\atm-active\\atm-daemon.exe `
  --cli target\\release\\atm.exe --daemon target\\release\\atm-daemon.exe --yes `
  --service atm-daemon
```

Creating those symlinks may require Developer Mode or an elevated shell. If
they are unavailable, the script fails closed; do not replace installed
executables or introduce a second daemon.
