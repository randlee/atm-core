---
id: AG.25
title: Live Two Daemon Pair Proof For Unified Cross Host Delivery
status: complete
branch: feature/pAG-s25-live-two-daemon-proof
worktree: ../atm-core-worktrees/feature/pAG-s25-live-two-daemon-proof
target: develop
---

# Sprint AG.25 — Live Two Daemon Pair Proof For Unified Cross Host Delivery

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.25
worktree: ../atm-core-worktrees/feature/pAG-s25-live-two-daemon-proof
branch: feature/pAG-s25-live-two-daemon-proof
status: complete
estimated_scope: medium
```

## Goal

Prove the post-AG.18-AG.24 design works on real daemon pairs with one message
path, one ack path, and no hidden local fallback.

## Hard Dependencies

- AG.18 merged
- AG.19 merged
- AG.20 merged
- AG.21 merged
- AG.22 merged
- AG.23 merged
- AG.24 merged

## Exact Targets

- live validation rows owned by:
  - `CROSSHOST-UNIFY-8`
  - `CROSSHOST-UNIFY-9`
- localhost same-host
- self-IP same-host
- Mac-to-Mac cross-host
- Windows/macOS cross-host if available in the same release window

## Deliverables

- retained live evidence for:
  - localhost send / read / ack / reply-ack
  - self-IP send / read / ack / reply-ack
  - Mac-to-Mac send / read / ack / reply-ack
  - failed cross-host ack leaves source pending
  - no hidden local-mailbox fallback for remote targets
- final proof note naming exact daemon/client versions and commit under test

## Required Work

- rerun the full functional matrix only after AG.18-AG.24 merge
- verify both directions on real host pairs
- retain sender and receiver logs plus CLI JSON for every row

## Explicit Code Samples

```text
atm send <agent>@<team>.127.0.0.1 --requires-ack ...
atm send <agent>@<team>.<self-ip> --requires-ack ...
atm send <agent>@<team>.<other-mac-ip> --requires-ack ...
atm ack <message-id> <reply>
```

## This Sprint Does Not Close

- does not add any new product scope
- only closes live-proof items after AG.18-AG.24 are merged and green

## Acceptance Criteria

- localhost and self-IP proofs are green on the unified architecture
- real cross-host Mac-to-Mac send and ack both work in both directions
- failed cross-host ack leaves source pending until confirmed delivery
- no daemon dispatch fallback writes a remote-target message to a local mailbox

### Hard Merge Gate

- net LOC for any code changed while making proof fixes trends toward
  reduction or any increase is explicitly justified and QA-approved before
  merge
- every completion, validation, and QA verdict must report:
  - `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- quality-mgr QA must independently sweep for any new duplicate abstraction,
  wire shape, or parallel code path introduced by any proof-driven fix

## Required Validation

- `just test`
- `just lint`
- `git diff --stat <sprint-base-sha>..HEAD -- crates/`
- retained live artifacts from both daemons and both CLIs for every executed
  proof row
