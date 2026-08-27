# Changelog

## 1.4.4

- first kit-era release: the repository's publish surface is fully cut over to
  the installed `sc-publish` kit (pinned revision `42e0fcea`, 49 byte-copied
  files guarded by ADR-050); manifest-driven publishing recovery lands as
  canonical consumer install plus parity proof (AT.1) and legacy
  publish-surface deletion with explicit `deferred-until-<gate>` dispositions
  for paths that outlive their first-release receipts (AT.2). Per the
  forward-only ruling, `v1.4.3` is not republished through the kit; this tag
  carries the first kit-era publish proof (Phase AT)
- ship the opt-in mTLS peer-wire transport for cross-host messaging, with
  peer connection pooling, TLS session reuse, and admission-writer batching
  and hot-path guardrails validated by official published benchmark campaigns
  under the new benchmark data contract and `benchmark-run` skill (Phase AO2)
- migrate release engineering onto the shared `sc-publish` kit: rendered
  manifest contracts (`release/publish-artifacts.toml`,
  `release/publish-channel-contracts.toml`), installer parity checks, and
  version-lockstep validation delegated to the installed kit (Phase AS)
- retire the waived sc-boundary lint debt and turn `develop` CI green again:
  boundary code fixes, lint calibration, and the extracted ack/send write
  module (Phase AU)
- standalone fixes: Windows loopback ack-timing flake, reproducible Python
  bootstrap closure, graft receiver recovery, daemon-switch signing gate,
  optional `.atm.toml` activation, and CI trigger/bootstrap hardening

## 1.4.3

- recovery release: `v1.4.2` was abandoned as a release tag/GitHub Release
  after the tag was created pointing at a commit that predates a required
  fix. `cargo publish`'s isolated package-verification build failed for
  `agent-team-mail` because `crates/atm/src/commands/api.rs` embedded
  `docs/atm-http-runtime/openapi.yaml` via `include_str!` from outside the
  crate directory, so it was absent from the package tarball. PR #930 fixes
  this by packaging a byte-for-byte-guarded derivative at
  `crates/atm/openapi.yaml`, but it merged to `main` after the `v1.4.2` tag
  already existed, and tag mutation is not a permitted recovery path. The 11
  crates already published at `1.4.2` on crates.io remain there permanently;
  all 12 crates are republished at `1.4.3` from `release/v1.4.3` (cut from
  `main`, includes PR #930). All content below was originally authored for
  `1.4.2` and is unchanged except for this recovery note and the version
  number.
- ship the minimal Tokio/Axum HTTP runtime (`atm-http-runtime`), replacing
  hand-written synchronous HTTP framing for local and cross-host listeners
  (Phase AL)
- delete the legacy transport machinery made redundant by the Tokio runtime:
  legacy HTTP framing, local/peer transport workers, resend/replay machinery,
  and the obsolete cross-host subsystem; reset the daemon to a local-IPC-only
  singleton (Phase AM, deletion-only)
- land the decomposed template message and queryable-message line: durable
  template catalog, render-on-read, typed search and raw read-only analyst
  queries, generic `atm compose` workflow, optional workflow metadata with
  OpenTelemetry projection, and the AN.8 validation/query-routing matrix
  (Phase AN)
- add `atm teams remove-member` for authorized local roster removal
- converge daemon runtime session/pid observation into diagnostic-only
  heartbeat caching (Phase AJ)
- publish `hermes-atm` and `atm-graft` to PyPI for the first time: full
  manylinux/musllinux/Windows/aarch64 wheel and sdist pipeline, CPython
  3.11-3.14 compatibility, and repo-wide version-sync enforcement consolidated
  onto one canonical gate
- add `atm-error`, `atm-http-runtime`, and `atm-template-sc-compose` to the
  published crate set (12 crates total, up from 9); the last was added after
  release preflight found `atm-daemon-bootstrap` depends on it at runtime
  while it was still unpublished (issue #923, fixed in PR #924)

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
