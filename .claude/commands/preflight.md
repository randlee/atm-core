---
name: preflight
version: 0.1.0
description: Run the canonical ATM release preflight flow, or open a fix worktree/PR when blockers are found.
options:
  - name: --fix
    description: Create a fix worktree from `develop`, apply fixes, commit/push, and open a QA-gated PR to `develop`.
---

# /preflight command

Run the canonical ATM release preflight flow from the current repo state.

## Prompt

Run ATM release preflight for this repo. By default, execute the canonical
local validation command first, then complete the publisher-only checks that
are intentionally not script-covered. If `--fix` is present, create a fix
worktree from `develop`, apply the required corrections there, commit/push, and
open a PR to `develop` without auto-merge.

## Execution

Default mode:

```bash
just validate
```

Then complete the non-script checks documented in:

```text
docs/release-preflight-checklist.md
```

Specifically:

- confirm completed release notes were provided by `team-lead`
- if a preflight workflow run already exists, download `release-findings` and
  inspect `release/release-findings.json`
- confirm any required Homebrew / `winget` preconditions are in place before
  publish

`--fix` mode:

1. create a `sc-git-worktree` from `develop`
2. apply the required fixes in that worktree
3. run the relevant validation locally before closing the worktree task
4. commit and push the fix branch
5. open a PR to `develop`
6. require the normal QA gate; do not enable auto-merge

## Outputs

This command produces:

- a local preflight result from `just validate`
- any required agent-side notes about non-script-covered checks
- in `--fix` mode, a fix branch, pushed commit(s), and a PR to `develop`
