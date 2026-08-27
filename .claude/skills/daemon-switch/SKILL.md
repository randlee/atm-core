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
6. When peer-interface or trust configuration changes, use `restart` to reload
   the one selected daemon; never launch an extra process.
7. On macOS, `switch` and `restart` fail closed unless both selected `atm` and
   `atm-daemon` strictly verify with the stable Apple Development identifiers
   and the configured Apple team identifier. Build with `just build` (or
   explicitly run `python3 .just/sign_daemon_dev.py`) before switching. This
   enforces a matched, signed local build; it is separate from macOS Local
   Network Privacy authorization. Windows currently warns and skips signing;
   other platforms are no-ops.

## Commands

macOS requires the existing LaunchAgent label and plist. Linux and Windows use
their existing service name.

```sh
# Inspect selected CLI/daemon binaries and the live daemon version.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor

# Discover the actual managed label and its plist. Never substitute an
# invented `com.atm.daemon` label: the current installed release may use a
# versioned label.
launchctl list | rg 'com\.atm\.daemon'
find ~/Library/LaunchAgents -maxdepth 1 -name 'com.atm.daemon*.plist' -print

# Switch both Homebrew links to a branch build, controlled-restart one daemon.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py switch \
  --cli target/release/atm --daemon target/release/atm-daemon --yes \
  --service <actual-label> --launch-agent-plist ~/Library/LaunchAgents/<actual-label>.plist

# If the checked label was unloaded but `status --doctor` still reaches an old
# daemon, the selector has not changed the live process. Do not continue with
# a split pair. After verifying the label/plist above, rerun the same command
# with `--repair-orphan`; it SIGTERMs exactly one proven `atm-daemon` owner of
# the ATM socket, then starts the selected managed service.

# Restore the latest Homebrew formula targets, not a Cellar path.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py restore --yes \
  --service <actual-label> --launch-agent-plist ~/Library/LaunchAgents/<actual-label>.plist

# Reload changed peer configuration without switching the selected pair.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py restart --yes \
  --service <actual-label> --launch-agent-plist ~/Library/LaunchAgents/<actual-label>.plist

# Temporarily stop exactly the managed daemon without altering either selected
# binary.  This is for an explicitly authorized backup/restore workflow only;
# follow it with `restart --yes` and `status --doctor` after restoration.
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py quiesce --yes \
  --service <actual-label> --launch-agent-plist ~/Library/LaunchAgents/<actual-label>.plist
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

## Default scratch root (ADR-055)

A session created by this skill never accepts a raw daemon argument,
alternate endpoint/root, environment selector, service wrapper, or arbitrary
configuration edit (see the ADR-053 Decision section). `ATM_TEMP` is exactly
such an environment selector, so it was never a candidate for this skill's
overlay surface, and this remains true after ADR-055: both the ordinary
launch path and the `--peer-wire-security plaintext-test` overlay session
inherit the same daemon-resolved default scratch root
(`<std::env::temp_dir()>/atm-<uid>` on Unix, `<temp_dir()>\atm` on Windows) —
no launch-overlay change was needed for this. An operator who wants a
non-default scratch root sets `ATM_TEMP` in the managed service's own launch
environment directly (outside this skill), the same way any other daemon
environment variable would be set; this skill's typed overlay session
continues to carry only the mTLS/plaintext-test mode selector.
