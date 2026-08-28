---
title: Phase AE Plan
status: complete
branch: integrate/phase-AE
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-AE
---

# Phase AE Plan

## Goal

Make installed end-user documentation a first-class ATM release surface.

Phase `AE` adds the missing operator-facing documentation system that Phase
`AD` deliberately did not solve:

- one authoritative repo-owned source tree for end-user docs
- one installed copy under the ATM versioned install root
- concise `atm help` topic output that points to installed long-form docs
- mechanically validated relative links and fenced examples
- release/publisher gates that fail when the installed user-doc set is stale

The result should be an agent-usable document set that ships with ATM and can
be trusted without opening the repo or reading implementation docs.

## Baseline

- planning branch: `plan/phase-AE`
- execution integration branch: `integrate/phase-AE`
- prerequisite accepted line:
  - `develop` after Phase `AD` merged on `2026-07-11`
- default install-root assumption for local installs:
  - `~/.local/atm/<version>/`
- deterministic packaging validation root:
  - `target/phase-ae/staged-install-root/`
- required installed user-doc root:
  - `~/.local/atm/<version>/share/doc/atm/`
- required installed user-doc entrypoint:
  - `~/.local/atm/<version>/share/doc/atm/README.md`
- required binary/doc relationship:
  - installed `atm` binary at `~/.local/atm/<version>/bin/atm`
  - installed docs resolved executable-relative as `../share/doc/atm/`
- runtime state remains separate at:
  - `~/.atm/`

## Governing Documents

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/documentation-guidelines.md`
- `docs/adr/ADR-025-installed-user-documentation-surface.md`

## Active Issues

| ID | Title |
|---|---|
| `AE-DOCS-001` | No authoritative repo-owned end-user document corpus exists. |
| `AE-DOCS-002` | ATM installs no versioned long-form user docs. |
| `AE-DOCS-003` | `atm help` does not surface installed long-form docs. |
| `AE-DOCS-004` | Hook and nudge-template operator docs are incomplete and not installation-oriented. |
| `AE-DOCS-005` | Relative-link integrity and fenced example validity are not mechanically verified. |
| `AE-DOCS-006` | Publisher/release preflight does not prove user docs were reviewed for the release version. |
| `AE-DOCS-007` | No phase-close artifact proves installed docs actually ship in release outputs. |

See [`issues.md`](./issues.md) for sprint mapping.

## Design Rules

- long-form operator help lives in installed markdown, not new CLI commands
- `atm --help` remains clap-owned syntax help
- `atm help` remains the only ATM-owned conceptual-help command
- `atm help` must stay concise and point to installed docs instead of trying
  to inline full operator manuals
- installed-doc lookup is derived from the resolved installed `atm` binary
  location, not from `ATM_HOME`
- the authoritative repo source tree for installed user docs is
  `docs/user-documents/`
- installed user docs must remain end-user facing:
  - no SQLite queries
  - no direct database edits
  - no repo-internal development workflow instructions
- `ATM_HOME` remains the runtime/data root only and must not be used to locate
  installed user docs
- all links between installed user docs must be relative so they survive the
  install-copy step unchanged
- fenced examples are production artifacts:
  - `json` must parse
  - `xml` must parse
  - `toml` must parse
  - `bash` must pass `bash -n`
- examples must show supported CLI usage and config only; speculative syntax
  is forbidden
- hook and nudge-template docs must list the exact supported variables and
  show complete copy-pastable examples
- publisher/release validation must fail closed when the installed user-doc set
  is missing, stale, or structurally broken
- sprint ownership is fixed as:
  - `AE.5` owns install-copy packaging, the deterministic staged install root
    at `target/phase-ae/staged-install-root/`, and the release-note wording
    that documents the installed doc location
  - `AE.7` owned the source-tree/installed-copy document-content validator;
    AT.2 retires it after the kit manifest superseded its legacy schema
  - `AE.9` owns only the accepted-line proof artifact and verification that the
    already-authored release notes still describe the installed doc surface

## Scope Rules

Phase `AE` may:

- add `docs/user-documents/` as the canonical repo-owned end-user doc tree
- install that tree into release/local-install outputs under
  `share/doc/atm/`
- update `atm help` wording so topic output points to installed docs
- add repo-local verification scripts/tests for link integrity, fenced example
  validity, and release-version freshness
- update publisher/release preflight so installed docs are part of the release
  gate

Phase `AE` must not:

- add new help-only CLI commands or flags beyond the retained `atm help`
  surface
- move developer architecture/requirements content into end-user docs
- document unsupported direct database manipulation as an operator workflow
- widen the product configuration surface beyond what ATM already supports

## Execution Order

1. [AE.1 User-Doc Contract And Source Tree Baseline](./sprint-AE1.md)
2. [AE.2 Setup And Identity Corpus](./sprint-AE2.md)
3. [AE.3 Mailbox And Diagnostics Corpus](./sprint-AE3.md)
4. [AE.4 Hooks And Nudge Template Corpus](./sprint-AE4.md)
5. [AE.5 Installed Copy Packaging](./sprint-AE5.md)
6. [AE.6 Help Surfacing](./sprint-AE6.md)
7. [AE.7 User-Doc Graph And Example Verification](./sprint-AE7.md)
8. [AE.8 Publisher Freshness Gate](./sprint-AE8.md)
9. [AE.9 Phase-End Installed-Docs Proof](./sprint-AE9.md)

Sprints execute back-to-back in merge-forward order:
`AE.1 -> AE.2 -> AE.3 -> AE.4 -> AE.5 -> AE.6 -> AE.7 -> AE.8 -> AE.9`

## Phase Exit Criteria

Phase `AE` closes only when all of the following are true on the accepted
line:

- `docs/user-documents/README.md` exists and is the entry point for the
  installed user-doc corpus
- the corpus contains, at minimum:
  - `install-layout.md`
  - `quickstart.md`
  - `identity-and-team.md`
  - `mailbox-workflows.md`
  - `doctor-and-log.md`
  - `hooks.md`
  - `nudge-templates.md`
  - `troubleshooting.md`
- every user-doc file carries the accepted metadata header including the
  release-review version marker
- release/local-install outputs place the corpus under
  `<install-root>/share/doc/atm/`
- the installed primary entrypoint is `<install-root>/share/doc/atm/README.md`
- the accepted runtime lookup model for installed docs is executable-relative:
  from `<install-root>/bin/atm`, resolve `../share/doc/atm/`
- `atm help` topic output points users to the installed corpus rather than
  duplicating long-form guidance inline
- all relative links between user-doc files are mechanically verified
- all fenced `json`, `xml`, `toml`, and `bash` blocks are mechanically
  verified
- publisher/release preflight fails closed when the corpus is stale or missing
- the phase-close artifact proves the installed archive/output contains the
  expected user-doc tree and that the copied links/examples still validate
- the accepted-line proof artifact is
  `reports/smoke/phase-AE-installed-docs-proof.md`
