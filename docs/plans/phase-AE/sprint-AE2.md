---
id: AE.2
title: Setup And Identity Corpus
status: planned
branch: feature/pAE-s2-setup-and-identity-corpus
worktree: ../atm-core-worktrees/feature/pAE-s2-setup-and-identity-corpus
target: integrate/phase-AE
---

# Sprint AE.2 — Setup And Identity Corpus

## Goal

Author the installed user-doc entry path for setup, install layout, and caller
identity/team usage.

## Hard Dependencies

- `AE.1` complete
- `docs/plans/phase-AE/plan-phase-AE.md`

## Exact Targets

- `docs/user-documents/README.md`
- `docs/user-documents/install-layout.md`
- `docs/user-documents/quickstart.md`
- `docs/user-documents/identity-and-team.md`
- `docs/user-documents/examples/quickstart/`
- `docs/user-documents/examples/identity/`

## Deliverables

- `README.md` is the entry point and links to every sibling document with
  relative paths
- `install-layout.md` explains:
  - install root vs runtime state
  - where binaries live
  - where installed docs live
  - what belongs under `~/.atm/`
- `quickstart.md` explains:
  - how to run `atm doctor`
  - how to send, read, ack, and peek with supported CLI syntax only
  - how to find the installed docs from the install root
- `identity-and-team.md` explains:
  - when `ATM_IDENTITY` and `ATM_TEAM` are required
  - environment vs explicit CLI arguments
  - the owner-only mutation rule
  - why `peek` and `list` differ from mutating commands
- the document set includes working fenced examples for:
  - `bash`
  - `json`

## Acceptance Criteria

- setup/identity docs contain no repo-internal developer instructions
- identity/team guidance is consistent with the owner-only mutation model from
  Phase `AD`
- every example uses supported CLI surface only and does not rely on direct
  SQLite access

## Required Validation

- `find docs/user-documents/examples/identity -name '*.json' -print0 | xargs -0 -n1 python3 -m json.tool >/dev/null`
- `find docs/user-documents/examples/quickstart docs/user-documents/examples/identity -name '*.sh' -print0 | xargs -0 -n1 bash -n`
- `git diff --check`
