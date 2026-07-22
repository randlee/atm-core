---
status: complete
branch: feature/pAI-1-daemon-preag-reset
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pAI-1-daemon-preag-reset
---

# Sprint AI.1: daemon reset to singleton/local-IPC (DAEMON-PREAG-RESET-1)

## Motivation

The Phase AG cross-host ladder (AG.16-AG.25) was ruled an over-engineered
dead end. `fix/daemon-pre-ag-deletion-reset` (PR #590, targeting `develop`)
implemented the corrective reset — deleting the entire cross-host/peer-transport
subsystem and returning the daemon to a local-IPC-only singleton — but PR #590
carried 5 iterative QA-fix-round commits as churn.

`integrate/phase-AI` is the new Phase AI integration line (branch off
`develop`). This sprint, AI.1, brings PR #590's final tree state into that
line as one clean commit via `git merge --squash`, deliberately not replaying
the QA-fix-round history, and closes the 3 findings QA-4 left open on PR #590.

This sprint doc is authored retroactively as an ad hoc corrective task, the
same pattern used for `DAEMON-PREAG-RESET-1`'s own dev cycle (no formal sprint
doc existed for that either) — the PR body and QA-4's findings are the
authoritative prior record; this doc exists only to satisfy the sprint-doc
gate for QA dispatch.

## Deliverables

1. Squash-merge `fix/daemon-pre-ag-deletion-reset`'s final tree state into
   `feature/pAI-1-daemon-preag-reset` as one commit, excluding `.triage/*.md`
   churn files. — **done** (f56ce35e)
2. Delete the cross-host/peer-transport subsystem: `peer_transport`,
   `claude_compat`, `boundary_adapters`, `direct_boundaries`, the
   `SourceIngress`/`ProjectionExport` boundary contracts, and their
   `replay_store`/config-layer supporting code. — **done**, carried from the
   squash (`boundaries/atm-daemon/peer-client-transport.toml`,
   `crates/atm-core/src/boundary/runtime.rs`,
   `crates/atm-daemon/src/boundary_adapters.rs`,
   `crates/atm-daemon/src/claude_compat.rs`,
   `crates/atm-daemon/src/direct_boundaries.rs`,
   `crates/atm-daemon/src/peer_transport.rs`,
   `crates/atm-runtime/src/replay_store.rs` all deleted)
   The retained `SourceIngress` and `ProjectionExport` TOML/doc records must
   be historical `state = "retired"` records with no live caller or accepted
   runtime contract, not renamed implementations.
3. Close the 3 findings QA-4 left open on PR #590:
   - `ARCH-003`/`RBQA-F008`: rewrite `docs/atm-daemon/boundaries.md`'s Policy
     Placement section from present-tense governance prose to retired/local-IPC-only
     framing. — **done**
   - `RBQA-F009`: add a defining `CHANGELOG.md` entry for the
     `DAEMON-PREAG-RESET-1` citation. — **done**
   - `RBQA-F010`: normalize retired-section blank-line spacing to the
     2-blank-line convention. — **done**
4. `cargo build --workspace` clean on the squashed branch. — **done**
5. Open PR #592 (`feature/pAI-1-daemon-preag-reset` -> `integrate/phase-AI`)
   superseding PR #590. — **done**
6. Canonical `.ttl` triage records for the 3 closed findings, committed on
   `integrate/phase-AI`. — **done** (bcfe0788)

## Acceptance Criteria

- Cross-host/peer-transport subsystem is fully absent from AI.1's tree. The
  retired `SourceIngress`/`ProjectionExport` records have no live caller,
  implementation, or accepted runtime contract.
- `docs/atm-daemon/boundaries.md` contains no present-tense cross-host
  governance prose or forward-looking `Phase Yb` language.
- `CHANGELOG.md` defines `DAEMON-PREAG-RESET-1`.
- `cargo build --workspace` and `cargo test --workspace` pass on
  `feature/pAI-1-daemon-preag-reset`.
- `git diff --name-only develop...HEAD` contains no Phase AG plan artifact,
  generated gate material, or unrelated triage change; the changed tree is
  limited to singleton/local-IPC baseline, deletion, required retirement docs,
  and their validation.
- CI green on PR #592.

## Retained contract

AI.1 retains only singleton ownership, local IPC, the dispatcher, storage
trait assembly, and the post-write event boundary. It introduces no peer
adapter, replay state, second request type, or alternate write path.
`atm_storage::validate_path_segment` remains the one inherited identifier
segment validator for AI.5; AI.1 neither forks nor replaces it.

## References

- PR #590 (superseded): `fix/daemon-pre-ag-deletion-reset` -> `develop`
- PR #592 (this sprint): `feature/pAI-1-daemon-preag-reset` -> `integrate/phase-AI`
- Triage records: `.triage/phase-AI/findings/ARCH-003-RBQA-F008.ttl`,
  `.triage/phase-AI/findings/RBQA-F009.ttl`, `.triage/phase-AI/findings/RBQA-F010.ttl`
