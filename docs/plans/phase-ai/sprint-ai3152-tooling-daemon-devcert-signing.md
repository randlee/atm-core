---
title: AI3152-TOOLING daemon development-certificate signing
status: complete
branch: feature/daemon-devcert-signing
worktree: ../atm-core-worktrees/feature/daemon-devcert-signing
target: integrate/phase-ai-31-33
---

# AI3152-TOOLING — daemon development-certificate signing

## Goal

Make local macOS daemon builds usable by the development harness when the
`atm-daemon-dev` signing identity is installed, while keeping normal builds
portable and silent on hosts without that identity.

## Scope

- `.just/sign_daemon_dev.py` checks the macOS keychain for the exact
  `atm-daemon-dev` identity and force-signs existing debug and release daemon
  binaries.
- The helper is invoked after `cargo build --workspace` in `Justfile`.
- Non-macOS hosts, missing `security`/`codesign`, absent identities, and
  signing failures are all silent no-ops.
- No daemon runtime, mTLS, cross-host, or release-signing behavior changes.

## Verification

- `.just/tests/test_sign_daemon_dev.py` covers platform gating, exact identity
  matching, both target paths, build wiring, and silent command failures.
- `just build`, `just lint`, and `just test` are the acceptance gates for this
  tooling-only change.
