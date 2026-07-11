---
id: AE.3
title: Mailbox And Diagnostics Corpus
status: planned
branch: feature/pAE-s3-mailbox-and-diagnostics-corpus
worktree: ../atm-core-worktrees/feature/pAE-s3-mailbox-and-diagnostics-corpus
target: integrate/phase-AE
---

# Sprint AE.3 — Mailbox And Diagnostics Corpus

## Goal

Author the installed user-doc set for mailbox workflows, diagnostics, and
operator troubleshooting.

## Hard Dependencies

- `AE.2` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `docs/user-documents/mailbox-workflows.md`
- `docs/user-documents/doctor-and-log.md`
- `docs/user-documents/troubleshooting.md`
- `docs/user-documents/examples/mailbox/`
- `docs/user-documents/examples/diagnostics/`
- `docs/user-documents/examples/troubleshooting/`

## Deliverables

- `mailbox-workflows.md` explains supported workflows for:
  - `send`
  - `list`
  - `peek`
  - `read`
  - `ack`
  - `clear`
- `doctor-and-log.md` explains:
  - `atm doctor`
  - how to interpret high-signal diagnostic output
  - where retained logs live
  - supported log-inspection commands
- `troubleshooting.md` explains supported recovery guidance for:
  - unresolved caller identity/team
  - daemon startup/connect failures
  - post-send warning surfaces
  - nudge delivery misconfiguration
- the document set includes working fenced examples for:
  - `bash`
  - `json`

## Acceptance Criteria

- mailbox workflow docs reflect the retained split:
  - `peek`/`list` are inspection-only
  - `read`/`ack`/`clear` are owner-only mutating commands
- troubleshooting guidance references supported ATM commands and log files only
- no troubleshooting step asks the operator to edit the database directly

## Required Validation

- `find docs/user-documents/examples/mailbox -name '*.json' -print0 | xargs -0 -n1 python3 -m json.tool >/dev/null`
- `find docs/user-documents/examples/diagnostics -name '*.json' -print0 | xargs -0 -n1 python3 -m json.tool >/dev/null`
- `find docs/user-documents/examples/mailbox docs/user-documents/examples/diagnostics docs/user-documents/examples/troubleshooting -name '*.sh' -print0 | xargs -0 -n1 bash -n`
- `git diff --check`
