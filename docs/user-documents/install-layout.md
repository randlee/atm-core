---
title: Install Layout
audience: end-user
reviewed_for_release: 1.6.0
---

# Install Layout

ATM separates installed program files from runtime state.

## Release-Channel Layouts

Install roots are selected by the release channel, not by a universal ATM
prefix:

- A GitHub release archive contains `bin/` and `share/doc/atm/`.
- Homebrew installs the bundled documentation under its `pkgshare` location.
- Winget and Scoop choose their installation locations from their published
  manifests.
- The prerelease installer uses its configured root, normally
  `~/.atm-builds/vX.Y.Z`, with `bin/` and `share/doc/atm/` below that version.

For an archive or prerelease install, the long-form entrypoint is
`share/doc/atm/README.md` next to the extracted `bin/atm` binary.

## Runtime State

Daemon coordination and durable mailbox state are host-scoped under the OS
account's `~/.atm/` root. The managed daemon and its database are shared by
that OS account rather than selected per workspace.

`ATM_HOME` remains a workspace/config discovery input. It does not select a
different daemon, daemon endpoint, database, or retained-log root.

Runtime state is not the installed documentation tree. Do not treat `~/.atm/`
as the source for long-form help content.

Common runtime-state examples:

- ATM daemon/runtime state
- mailbox and roster data
- host-scoped ATM logs

## Relative Doc Layout

ATM documentation is authored so that relative links continue working after the
copy into the installed `share/doc/atm/` tree.

For an archive or prerelease installation, the long-form doc entrypoint is the
adjacent relative path `../share/doc/atm/README.md` from `bin/atm`. Homebrew,
Winget, and Scoop use their channel-managed locations instead.

Return to the [ATM User Guide](./README.md).
