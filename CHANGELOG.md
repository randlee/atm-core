# Changelog

## Unreleased

- add `atm teams remove-member` for authorized local roster removal
- `DAEMON-PREAG-RESET-1`: reset the daemon to a local-IPC-only singleton by
  deleting the entire cross-host/peer-transport subsystem (`peer_transport`,
  `claude_compat`, `boundary_adapters`, `direct_boundaries`, the
  `SourceIngress`/`ProjectionExport` boundary contracts, and their
  `replay_store`/config-layer supporting code), following the corrective
  ruling that the prior cross-host ladder (`AG.16`-`AG.25`) was an
  over-engineered dead end. Sprint `AI.1` carries this reset forward as the
  Phase AI cross-host baseline.

## 1.3.0

- complete the `AD.13` through `AD.30` corrective line by tightening caller
  identity ownership, restoring direct post-send emission, and deleting
  retired daemon-side Claude/reconcile paths
- restore Windows daemon CI depth coverage for same-host local IPC shutdown,
  injected accept-failure, and post-terminate rejection without accepting
  flaky or hang-prone test behavior
- converge the Phase AD messaging protocol: mailbox peek surface, owner-only
  mutation reset, self-addressed send rejection, self-ack loop termination,
  and historical poison cleanup
- close out the rusqlite storage/core coupling remediation and the
  daemon-bootstrap boundary drift plan

## 1.2.3

- recover the interrupted `v1.2.2` publish by cutting a clean `release/v1.2.3`
  line from the immutable `3075384e` tag point rather than mutating the
  partial `v1.2.2` release in place
- add the missing `description` metadata to `atm-storage-claude` so crates.io
  accepts the crate during ordered publish
- replace bash-4-only `mapfile` usage in `release.yml` with bash-3.2-compatible
  loops so the macOS archive jobs package release binaries successfully

## 1.2.2

- converge the Phase AC storage layer on the `atm-storage` contract and
  canonical types, unifying the SQLite backend and the RPC envelope/domain
  types on a single shared type surface
- close out storage cleanup and deletion handling and prove SQL Server
  readiness for the storage backend contract
- land the Phase AC production-readiness boundary fixes so the retained
  runtime enforces the storage boundary through live factory wiring
- consolidate release readiness (PR #425): `release_gate.sh` branch-regex
  enforcement, the canonical release validation suite (`validate_release.py`,
  `verify_release_archive.py`), publisher release-branch discipline, and the
  publishing-improvements plan docs

## 1.2.1

- restate the daemon architecture and introduce subsystem doctor traits for
  health/observability of daemon subsystems
- clean up the mailbox path and remove the SQLite legacy-compatibility surface
- relock and enforce the crate boundaries hardened during Phase AA
- upgrade observability across the daemon subsystems

## 1.2.0

- complete Phase Z release validation, including fast/normal/thorough smoke,
  `atm-dev` canary and dogfood, final release sign-off, and retained-log
  maintenance adoption through `sc-observability` `v1.1.0`
- validate the same-host `atm-graft` ICD path in thorough smoke and carry the
  final READY release verdict into the authoritative Phase Z readiness records

## 1.1.2

- add the first production-readiness hardening follow-up line for shared-host
  behavior, timeout/retry clarity, and retained-log background maintenance
- land coverage reporting and the smoke execution/reporting skill line used for
  fast, normal, and thorough release validation

## 1.1.1

- preserve Claude inbox files in JSON array format during ATM shared-inbox writes
  so ATM-authored messages inject into live Claude sessions correctly
- keep ATM machine metadata under `metadata.atm` for supported fields while
  leaving alert fields on their current top-level compatibility shape for this
  sprint
- keep forward `metadata.atm.messageId` values as real ULIDs assigned by ATM
  send/ack flows rather than deriving them from legacy UUID compatibility ids
