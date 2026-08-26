---
title: Install Layout
audience: end-user
reviewed_for_release: 1.4.4
---

# Install Layout

ATM separates installed program files from runtime state.

## Installed Files

The installed binary lives under a versioned install root:

- `bin/atm`
- `bin/atm-daemon`
- `share/doc/atm/README.md`

Long-form user docs live under `share/doc/atm/`.

A typical local install layout looks like this:

```text
~/.local/atm/1.4.3/
  bin/
    atm
    atm-daemon
  share/
    doc/
      atm/
        README.md
        quickstart.md
        identity-and-team.md
        ...
```

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

If you know the installed ATM binary path, the long-form doc entrypoint is the
adjacent relative path `../share/doc/atm/README.md`.

Return to the [ATM User Guide](./README.md).
