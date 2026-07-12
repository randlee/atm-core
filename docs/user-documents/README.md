---
title: ATM User Guide
audience: end-user
reviewed_for_release: 1.3.0
---

# ATM User Guide

This directory is the installed long-form user documentation for ATM.

When ATM is installed, this tree is copied to `share/doc/atm/` next to the
versioned `atm` binary. These documents are for operators and agents using ATM.
They are not developer architecture notes.

## Start Here

- [Install Layout](./install-layout.md)
- [Quickstart](./quickstart.md)
- [Identity And Team](./identity-and-team.md)
- [Mailbox Workflows](./mailbox-workflows.md)
- [Doctor And Log](./doctor-and-log.md)
- [Hooks](./hooks.md)
- [Nudge Templates](./nudge-templates.md)
- [Troubleshooting](./troubleshooting.md)

## Scope

These documents describe supported ATM usage only:

- command-line workflows
- hook and nudge configuration
- install layout and runtime-state separation
- operator-facing diagnostics

These documents do not define developer implementation boundaries, direct
SQLite editing steps, or repo-internal release procedures.
