# Changelog

## Unreleased

## 1.5.0

- Phase AV mailbox-read serialization fix: dedicated reader lanes for mailbox
  and search queries with bounded pools and queue depth, so reads no longer
  serialize behind the writer lane; `atm doctor --json` now publishes the
  effective `reader_lanes` contract (Phase AV, #1120, #1143)
- release-readiness gate: the 1.4.7-1.4.13 readiness campaign on the
  isolated atmbench host and Windows, with smoke/benchmark evidence and
  benchmark floors recorded under `site/reports/` and triage records under
  `.triage/readiness-1.4/` (#1141, #1150, #1151-#1155, #1159, #1167-#1169)
- fix(http-runtime): dial direct peers by their routable address with a
  short-lived resolution cache and surface the connect cause in the CLI
  (#1142)
- fix(peer-tls): fail closed on legacy literal-IP trusted-peer rows and print
  the exact migration remediation (#972, #1138)
- fix(storage): preserve the raw SQLite cause on mapped errors and print a
  `Cause:` line in the CLI (#1134); one-time rebuild relaxing the legacy
  `mail_messages.message_text NOT NULL` constraint so template sends work on
  databases created before 2026-08-11 (#1135)
- fix: retain OS causes for retained-log startup failures (#1130, #1131) and
  harden the daemon observability boundary in `atm-observability` (#1133)
- fix: support a pinned self-signed daemon-dev signing identity (Phase AS,
  #1127)
- fix(smoke): validated `ATM_SMOKE_DIRECT_PEER_PORT` override for the
  direct-peer smoke rows (#1152)
- fix(bootstrap): Windows `just bootstrap` selects the pinned Python 3.14
  instead of the first `python.exe` on PATH (#1165)
- lint: guard SQLite writer platform neutrality (ADR-007, #1107)
- chore: upgrade the sc-compose ecosystem pins to 1.6.1 (#1129); centralize
  workspace crate versioning (#1122)
- document that release archives nest their binaries under a top-level
  `atm_<version>_<target-triple>/` directory (for example,
  `atm_1.4.6_aarch64-unknown-linux-gnu/bin/atm`) rather than a flat archive
  root (Phase AS, #1105)
- enforce the request-budget ordering between SQLite storage, the replacement
  daemon, and same-host clients; rebuild local TCP and Unix transports after a
  daemon generation change, classify pre-send reconnects separately from
  writes whose delivery may have completed, and preserve fail-closed recovery
  guidance for uncertain requests (Phase AQ, AQ1.9)
- publish a native `aarch64-unknown-linux-gnu` release archive (arm64
  Linux, e.g. Apple-Silicon colima/docker containers without emulation);
  Homebrew arm-linux is not yet wired (#1057)
- add the `atm queue` CLI verb (mirrors `atm send`'s full surface, including
  `--attach`, with the recipient nudge deferred rather than fired
  immediately) plus the nudge taxonomy and `PendingNudgeStore` storage
  contract that every later Phase AQ sprint builds on (ADR-054: nudge as the
  umbrella term, steer/queue as the two kinds) (Phase AQ, AQ1)
- wire `atm-graft` dual-channel queue delivery to an actual delivery
  trigger: harness idle-signal heartbeats and a bare-CLI Stop-pull path
  backed by a bounded, per-member in-memory FIFO, so a deferred queue nudge
  is guaranteed to drain even for a receiver with no persistent daemon
  session (Phase AQ, AQ2 + AQ2.5)
- add Herdr as a second, selector-gated local steer backend (`atm-herdr`)
  alongside retained tmux, with its own health/circuit breaker (ADR-058)
  (Phase AQ, AQ2.6)
- add the Herdr poll-gated queue-wake pump: a fixed-cadence, roster-wide
  Herdr session poll that drains pending queue messages on `RuntimeHealth`
  idle transitions (Phase AQ, AQ2.7)
- add tmux idle-drain (one queued nudge per idle transition) plus a
  kind-agnostic recovery sweep that catches missed or crashed drains
  (Phase AQ, AQ3)
- ship ATM Send-To core: `atm send --attach`/`--from-json`, the `ATM_TEMP`
  scratch-directory contract with a 30-day TTL sweeper, and per-host
  cross-host transfer scripts (`scripts/transfer/sftp.sh`, `sftp.ps1`)
  (ADR-055) (Phase AQ, AQ4)
- ship the ATM Send-To human-visible surface: one-gesture per-OS shell entry
  points for Finder/Explorer/Nautilus, the reference and native member
  pickers, and the upstream Wyvern contract-test filing (wyvern#139,
  wyvern#140) (Phase AQ, AQ5)
- add the sc-ecosystem dependency release-preflight gate: pin-latest checks
  for Wyvern/sc-compose/sc-observability plus their integration tests, run
  before every release (Phase AQ, AQ6)
- add the hermes-atm wheel verification harness: rebuilt wheel plus an
  automated restart-matrix crash-guard suite (the live m5 restart-matrix run
  remains an open follow-up, `AQ1.9-m5`) (Phase AQ, AQ1.9)

## 1.4.5 (graft-registration slice of Phase AQ only; see Unreleased for the remainder)

- complete the Phase AQ graft registration cutover: `atm-graft` now uses the
  daemon registry lease and same-host flock instead of the retired endpoint
  record, with the `atm-graft` Python wheel rebuilt against ADR-056

## 1.4.4

- first kit-era release: the repository's publish surface is fully cut over to
  the installed `sc-publish` kit (pinned revision `25668ecc`, 54 byte-copied
  files guarded by ADR-050, recorded in `release/sc-publish-pin.toml`);
  manifest-driven publishing recovery lands as
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
