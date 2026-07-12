---
title: Phase AE Issues
status: planned
branch: plan/phase-AE
worktree: /Users/randlee/Documents/github/atm-core-worktrees/plan/phase-AE
---

# Phase AE Issues

## Issue Inventory

| ID | Title | Closed by |
|---|---|---|
| `AE-DOCS-001` | No authoritative repo-owned end-user document corpus exists. | `AE.1`, `AE.2`, `AE.3`, `AE.4` |
| `AE-DOCS-002` | ATM installs no versioned long-form user docs. | `AE.5` |
| `AE-DOCS-003` | `atm help` does not surface installed long-form docs. | `AE.6` |
| `AE-DOCS-004` | Hook and nudge-template operator docs are incomplete and not installation-oriented. | `AE.4` |
| `AE-DOCS-005` | Relative-link integrity and fenced example validity are not mechanically verified. | `AE.7` |
| `AE-DOCS-006` | Publisher/release preflight does not prove user docs were reviewed for the release version. | `AE.8` |
| `AE-DOCS-007` | No phase-close artifact proves installed docs actually ship in release outputs. | `AE.9` |

## Closure Notes

- `AE.1` defines the corpus contract, metadata header, and required tree.
- `AE.1` also fixes the accepted corpus/install contract used by
  `integrate/phase-AE`:
  - repo-owned end-user docs live in `docs/user-documents/`
  - installed long-form docs ship under `share/doc/atm/`
- `AE.2` through `AE.4` author the actual end-user content in bounded groups.
- `AE.5` through `AE.8` make the corpus shippable, discoverable, and gated.
- `AE.9` is the only sprint allowed to claim the installed-doc release proof.
