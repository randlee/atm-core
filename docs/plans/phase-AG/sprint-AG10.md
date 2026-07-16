---
id: AG.10
title: Secured Cross-Host Transport Implementation
status: planned
branch: feature/pAG-s10-secured-transport
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-transport
target: develop
---

# Sprint AG.10 — Secured Cross-Host Transport Implementation

```yaml
plan_type: sprint_plan
phase: AG
sprint: AG.10
worktree: ../atm-core-worktrees/feature/pAG-s10-secured-transport
branch: feature/pAG-s10-secured-transport
status: planned
estimated_scope: medium
```

## Goal

Implement the actual secured daemon-to-daemon transport after AG.8 finishes the
security design/reconciliation work.

## Deliverables

- secured daemon-to-daemon transport implementation
- peer-auth / credential handling implementation
- secure loopback validation support
- secure LAN and routed/VPN validation support
- large-module lint remediation plan and execution constraints for the AG.10
  code line so AG.11+ can merge forward on smaller, stable files instead of
  repeating structural churn

## Refactor Tasks For Merge-Forward Stability

These tasks are required AG.10 recommendations and are intended to be landed as
behavior-preserving refactors. They exist to resolve `file too large` /
function-length / lines failures in a way that reduces conflict pressure on the
AG.11+ corrective line.

- split oversized transport runtime code by responsibility instead of by test
  accident:
  - move socket server accept/bind lifecycle into a dedicated server module
  - move handshake / authentication negotiation into a dedicated security or
    handshake module
  - move retry / receipt / outcome classification into a dedicated delivery
    outcome module
  - keep only top-level orchestration in the runtime entry module
- split oversized CLI/runtime command files by boundary:
  - parsing and normalization helpers separate from execution/orchestration
  - user-facing output formatting separate from dispatch logic
  - storage contract types separate from transport execution code
- prefer extracting sealed internal modules over widening public APIs
- preserve the AG.10 external behavior and wire contract while refactoring:
  - no new CLI syntax
  - no new environment-variable surface
  - no change in message envelope shape
  - no localhost-only fast path
- place extracted modules where AG.11+ can either reuse them directly or delete
  them cleanly without another large-file rewrite

## Refactor Targets

The AG.10 line should explicitly evaluate and, where practical, split these
large surfaces first because they are the highest merge-conflict and lint-risk
points for AG.11+:

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm/src/commands/send.rs`

Recommended split shape:

- `runtime_health.rs`
  - keep dispatch orchestration only
  - extract retry policy, receipt handling, and remote/local branch helpers
- `composition.rs`
  - keep composition root only
  - extract peer transport config loading / validation helpers
- `send/mod.rs`
  - keep public send entrypoints only
  - extract target parsing / normalization and delivery-result helpers
- `commands/send.rs`
  - keep CLI command wiring only
  - extract argument normalization and output rendering helpers

## Boundary And Type Contract

Illustrative implementation signatures:

```rust
pub struct PeerCredential {
    pub host_name: String,
    pub fingerprint_sha256: String,
    pub cert_not_after: Option<DateTime<Utc>>,
}

pub trait SecureTransportNegotiator {
    fn connect_secure(&self, endpoint: SocketAddr, credential: &PeerCredential) -> Result<SecurePeerChannel, AtmError>;
    fn accept_secure(&self, socket: AcceptedSocket) -> Result<AuthenticatedPeer, AtmError>;
}

pub trait PeerCredentialStore {
    fn local_identity(&self) -> Result<LocalPeerIdentity, AtmError>;
    fn trusted_peer(&self, host: &Hostname) -> Result<Option<PeerCredential>, AtmError>;
}
```

These names are illustrative, but the sprint requires equivalent explicit type
ownership for secure negotiation and credential lookup.

## Required Validation

- secure transport implementation follows AG.8's approved security direction
- no insecure fallback is taken silently
- secure loopback row `AG-VAL-012` executes on the implemented surface
- secure LAN host-pair rerun row `AG-VAL-013` executes on the implemented
  surface
- unauthorized/unauthenticated peer rejection row `AG-VAL-014` executes on the
  implemented surface
- secure routed/VPN host-pair rerun row `AG-VAL-015` executes on the
  implemented surface
- refactor commits are behavior-preserving:
  - no CLI surface drift
  - no wire-message drift
  - no new daemon/config control plane introduced as part of lint cleanup
- the refactor shape demonstrably reduces the merge-forward surface into AG.11+
  rather than creating additional top-level files with mixed responsibilities

## Unit-Test Plan

- peer-auth handshake rejection on bad or missing credentials
- encryption negotiation failure does not fall back silently to insecure mode
- loopback/local diagnostic surfaces remain explicitly scoped and do not
  accidentally widen remote trust
- extracted helper modules retain their current behavior through targeted unit
  coverage where splitting introduces private helper seams

## Integration-Test Plan

- secure daemon-to-daemon handshake succeeds for authorized peers
  (`AG-VAL-013`, `AG-VAL-015`)
- unauthorized or unauthenticated peer is rejected before mailbox mutation
  (`AG-VAL-014`)
- existing AG.7 functional rows still pass under the secured transport
  (`AG-VAL-013`, `AG-VAL-015`)
- at least one AG.10 integration path exercises code that now crosses the new
  internal module seams so the split is proven non-semantic

## Smoke-Test Plan

- secure loopback smoke (`AG-VAL-012`)
- secure LAN host-pair rerun (`AG-VAL-013`)
- secure routed/VPN host-pair rerun (`AG-VAL-015`)

## Entry Gate

- AG.8 planning/reconciliation work is complete

## Acceptance Criteria

- the sprint owns the actual secured-transport implementation rather than only
  planning prose
- a Boundary And Type Contract exists for the secure transport surface
- secure loopback, integration, and smoke coverage are all specified and
  required
- AG.10 large-file lint remediation is handled through responsibility-based
  internal module extraction, not by adding AG.11 corrective logic early
- the AG.10 refactor shape leaves AG.11+ free to layer the remote-target
  contract and same-host proofs on top without another broad file reshuffle
