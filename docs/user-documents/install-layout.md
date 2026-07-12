---
title: Install Layout
audience: end-user
reviewed_for_release: 1.3.0
---

# Install Layout

ATM separates installed program files from runtime state.

## Installed Files

The installed binary lives under a versioned install root:

- `bin/atm`
- `bin/atm-daemon`
- `share/doc/atm/README.md`

Long-form user docs live under `share/doc/atm/`.

## Runtime State

Runtime state lives under `~/.atm/`.

Runtime state is not the installed documentation tree. Do not treat `~/.atm/`
as the source for long-form help content.

## Relative Doc Layout

ATM documentation is authored so that relative links continue working after the
copy into the installed `share/doc/atm/` tree.

Return to the [ATM User Guide](./README.md).
