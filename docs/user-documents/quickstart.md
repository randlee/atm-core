---
title: Quickstart
audience: end-user
reviewed_for_release: 1.6.0
---

# Quickstart

Use this guide when you need the smallest working ATM path.

## Basic Flow

1. Confirm your repo is ATM-enabled and you know the current team.
2. Run `atm doctor` first when you need a health/config check.
3. Send a message with `atm send`, or preview template-backed content with
   `atm compose` before sending it.
4. Inspect queues with `atm list` or `atm peek`.
5. Read messages with `atm read`.
6. Acknowledge a pending-ack message with `atm ack`.

## Minimal Commands

Environment-driven caller context:

```bash
export ATM_TEAM=atm-dev
export ATM_IDENTITY=arch-ctm
atm doctor
atm send quality-mgr@atm-dev "review the current branch"
atm peek quality-mgr@atm-dev --team atm-dev --as quality-mgr
atm read --team atm-dev
atm ack 01KRFK5QTF2R6NRS3Q0F8Z9K0S "received"
```

## Template-Backed Messages

For generated content, preview the exact rendered body locally before
delivery. `atm compose` does not read or write a mailbox; `atm send --template`
uses the same template and variables for delivery.

```bash
atm compose --template notice.j2 --vars notice-vars.json
atm send quality-mgr@atm-dev --template notice.j2 --vars notice-vars.json
```

Cross-agent inspection without mutation:

```bash
atm list quality-mgr@atm-dev --team atm-dev --as quality-mgr --json
atm peek quality-mgr@atm-dev --team atm-dev --as quality-mgr --json
```

The command examples in this guide use only supported CLI surfaces. They do
not rely on direct database access or private files under `~/.atm/`.

## Installed Docs By Release Channel

A GitHub archive contains `bin/atm` and `share/doc/atm/README.md`. A
prerelease install uses `~/.atm-builds/vX.Y.Z` as its configured root with the
same relative paths. Homebrew installs the bundled docs under `pkgshare`; use
the published Winget or Scoop manifest for that channel's managed location.
See [Install Layout](./install-layout.md) for the complete channel distinction.

## Next Documents

- identity and caller resolution: [Identity And Team](./identity-and-team.md)
- queue inspection and message handling: [Mailbox Workflows](./mailbox-workflows.md)
- diagnostics: [Doctor And Log](./doctor-and-log.md)
- durable task lifecycle: [Tasks](./tasks.md)
- native-IPC Herdr members: [Herdr Integration](./herdr.md)
- shell-ready examples: [examples/quickstart/](./examples/quickstart/)

Return to the [ATM User Guide](./README.md).
