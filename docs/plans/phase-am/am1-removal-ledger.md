# AM.1 Legacy Transport Removal Ledger

Status: refreshed draft inventory only (2026-08-10).  This is **not** a
deletion, guard-activation, or runtime-change authorization.  It was refreshed
on `feature/pam-s1-removal-ledger` after merging `origin/integrate/phase-am`
and `origin/integrate/phase-al`; the latter was refreshed to `5c18aeb2` for
the final reassessment.

Phase AL has merged to `develop` (PR #826/#827, 2026-08-10).  `ATM-QA-002-AL9`
-- the finding tracking the M5/cross-host row's disposition -- is closed:
its trigger (a frozen final `integrate/phase-al` candidate SHA, before
proposing to `develop`) has passed.  The remaining M5 physical-artifact
capture (cross-host write, Windows TCP benchmark, one-listener/one-publisher
proof) is a distinct, already-scheduled final evidence pass owned by
`team-lead`/`arch-ctm`-as-M5-operator, deliberately deferred per the user's
standing no-new-smoke-tests-until-final-review policy.  It is tracked
separately (see `ATM-QA-004-AL9` and the AL9 final-evidence-pass docs) and is
**not** a per-sprint gate on AM.2 through AM.6 -- do not re-raise it as a
blocking AM finding.  The Tokio/Axum runtime remains the active proof
subject; the legacy `atm-daemon` is an AM removal subject and must not be
started as a shortcut for evidence.

AM.2 and AM.3 are merged and closed on this basis, by explicit user
direction.  Treat that as the standing precedent for AM.4 through AM.6:
proceed on real code-level dependencies (ledger rows, caller-before-callee
order) without re-opening the AL.9-evidence question per sprint.  The
eventual formal "AM.1 freeze" (naming the final evidence-pass SHA once
captured) is a closeout bookkeeping step, not a precondition for this
sprint's deletion work.

The authoritative AM.1 sprint document deliberately has no frontmatter.  The
dispatch's generic frontmatter-completion criterion therefore does not apply;
this document remains a draft until the named AM owner accepts a freeze.

## Refresh task list

- [x] Merge the current AM and AL integration histories into this dedicated
  branch before inventorying it.
- [x] Re-read the AM plan, AM.1 sprint, AL/AM boundary checklist and transition
  document; retain the no-deletion/no-early-guard boundary.
- [x] Recheck current source, Cargo callers, test fixtures, architecture gates,
  AL.9/AL.13 evidence contracts, AL.17/AL.19 graft boundaries, and AL.11
  shutdown disposition.
- [x] Replace stale file and call-edge claims below with current paths.
- [x] Execute the draft guard's mutation suite and a currently-empty category;
  keep the guard unregistered while retained categories are non-empty.
- [ ] AM owner: wait for team-lead's final phase-AL evidence-pass acceptance,
  then record its candidate SHA and artifact links before freezing this ledger.
  Current checked-in evidence still does not prove that acceptance.

## AM.1--AM.6 critical-review task list

This is a review inventory, not authority to begin deletion before the final
phase-AL evidence pass is accepted.  Each item must be resolved in the named
sprint-plan document and rechecked against the frozen graph before its deletion
PR starts.

- [x] **AM-PLAN-001 — repair the raw-framing dependency direction.** AM.2 says
  it unblocks AM.3/AM.4, while AM.3 says its normal predecessor is AM.2.
  Current compiled local transports and `atm-daemon-client` still call raw
  framing, so caller-before-callee requires AM.3's applicable migrations and
  deletions before AM.2 removes `HttpFrameReader`.  Replace the contradictory
  numerical dependency prose with the frozen graph's explicit edges.
- [x] **AM-PLAN-002 — remove the stale AL.7 TLS premise from AM.4.** AM.4 says
  to retain “AL.7's TLS physical adapter” and require mTLS proof.  AL.9 records
  that AL.7 was never implemented and TLS is out of MVP scope.  AM.4 must name
  the actual retained canonical direct-peer path, if any, and must not require
  an unimplemented TLS adapter or turn the TLS quarantine into production scope.
- [x] **AM-PLAN-003 — distinguish active canonical direct-peer code from
  removable legacy peer grammar.** AM.4's “all peer-specific client/listener”
  wording is too broad: the AL final-evidence gate still requires the canonical
  direct-peer route.  Limit deletion to legacy DTO/header/body grammar and
  parallel ingress; identify each retained AL client/listener by current path.
- [x] **AM-PLAN-004 — rebaseline AM.5 against actual absence.** AM.5 confirms
  `peer_drain_coordinator`, `https_transport`, and
  `peer_delivery_observability` were already absent and takes no deletion
  credit for them. It removed the remaining unused outbound query/cursor
  pipeline and serialized peer replay payload, retained direct-host metadata,
  idempotence, and synchronous received-hook behavior, and enabled the
  resend/replay negative guard in `just lint`.
- [x] **AM-PLAN-005 — complete AM.1's per-row ownership data.** Every future
  deletion row needs exact production callers, AL replacement/retain rationale,
  explicit owner, Cargo edge, fixtures/docs, and validation.  The current
  ledger has the broad surfaces but does not yet map every orphan candidate by
  its owning AM.2--AM.5 PR.
- [x] **AM-PLAN-006 — complete the required observability/config inventory.**
  AM.1's sprint requires named consumers and retain/remove dispositions for
  capacity/state registries, doctor output, dashboards/events, and config keys.
  Preserve `active_connection_registry`, keep-alive admission, bounded
  singleton recovery, and canonical direct-send observations unless the frozen
  graph proves them obsolete; separately disposition stale replay-era scripts
  and documentation such as `scripts/smoke/analyze_logs.py`.
- [x] **AM-PLAN-007 — make the guard contract match its stated scope.** The
  draft script has representative symbol mutations and a clean direct-SQLite
  category, but it does not by itself map every prohibited module name or every
  orphaned dependency edge named by AM.1.  Decide which guarantees remain in
  `atm-architecture` boundary tests and which AM deletion PR must add to the
  selected guard category; retain mutation proof for each enabled rule.
- [x] **AM-PLAN-008 — define AM.6 closure from the frozen rows, not a generic
  “all legacy absent” claim.** AM.6 must list the final source, Cargo, fixture,
  documentation, guard, and smoke evidence for every frozen row, while keeping
  public JSON/schema compatibility and the sealed `DaemonApiClient` boundary
  unchanged.

AM.6 resolves AM-PLAN-005 through AM-PLAN-008 in
[`am6-closure-proof.md`](./am6-closure-proof.md): it maps every row to its
owner and retained/deleted evidence, records the observability/config
dispositions including `scripts/smoke/analyze_logs.py`, specifies the enabled
guard mutations, and defines closure from the frozen rows while preserving the
public JSON/schema and sealed `DaemonApiClient` contracts.

## Repeatable inventory commands

Run from the repository root before each correction or deletion PR:

```sh
rg -n 'HttpFrameReader|read_http_request|decode_request|write_http_request|read_http_response_with_frame_reader' crates
rg -n 'PEER_SOURCE_HOST_HEADER|PeerMessageArray|peer_sync_path_host|route_peer_http_request' crates
rg -n 'PeerDrainCoordinator|PeerDeliveryCoordinator|PeerDeliveryProjection|peer_delivery_observability|peer_delivery_router' crates
rg -n 'atm_daemon_client|atm-daemon-client|try_connect|exchange_request' crates --glob 'Cargo.toml' --glob '*.rs'
rg -n '^\\s*(use|extern crate)\\s+rusqlite|^\\s*rusqlite\\s*=' crates/atm-daemon crates/atm-http-runtime
rg --files crates | rg '(peer|tls|https)'
cargo tree -i atm-daemon-client
```

The `atm-peer-tls-interop` and `atm-storage/src/tls.rs` paths now exist and are
quarantined/reference-only AL artifacts.  They are not absent paths and are not
AM.1 deletion targets without a future, accepted reference graph proving an
incoming legacy edge.

## Current production removal ledger

| ID | Current legacy surface and incoming edges | Disposition / owner | Validation and caller-before-callee order |
| --- | --- | --- | --- |
| AM1-RM-001 | **Removed by AM.2.** The raw frame reader plus request/response stream helpers and core request decoder are deleted. | `atm-daemon-client` now delegates retained compatibility calls to the typed runtime client; `atm-http-runtime::message_handler` owns framework request conversion.  The raw-framing negative guard is enabled for the empty category. | `python3 scripts/phase-am/check_legacy_transport_removal.py --category raw-framing`; workspace search; `just test`; `just lint`; M5 fast smoke (`20260810T195635850095Z-pid13391-smoke-fast`).  Follows RM-002 and RM-003. |
| AM1-RM-002 | `atm-daemon-client` synchronous local client: `http_exchange`, `try_connect`, `exchange_request`, compatibility/bootstrap support.  `cargo tree -i` and caller search confirm `atm` and `atm-graft` still use it only for daemon availability plus non-write read/ack/admin compatibility dispatch. | **AM.2 migration owner.** Preserve this public non-write compatibility contract while replacing its raw framing with the shared typed HTTP client; do not migrate canonical writes back into it.  It was not an AM.3 deletion target, but no unnamed later owner remains. | `cargo tree -i atm-daemon-client`; caller search in `crates/atm` and `crates/atm-graft`; no write path may select this wrapper; raw-framing inventory must be empty after AM.2. |
| AM1-RM-003 | Legacy `atm-daemon` local listener code: `local_ipc_transport` submodules, `local_tcp_transport`, `local_ipc_connection`, transport-specific request/connection workers, their frozen composition, and local-worker fixtures. | **Removed by AM.3.** The shipped `atm-daemon` binary already selects `atm-daemon-bootstrap` and `atm-http-runtime` as its sole listener path.  Generic runtime health, ownership, and shutdown concerns remain outside this deleted listener family. | Before/after path and raw-symbol search; `cargo test -p atm-architecture --test boundary_enforcement`; `just lint`; `just test`.  RM-003 now precedes RM-001. |
| AM1-RM-004 | **Removed by AM.4.** The public `PEER_SOURCE_HOST_HEADER` marker is deleted from `atm-core`; no live `PeerMessageArray`, peer-sync route, or `route_peer_http_request` source exists. | `atm-http-runtime` retains only a private boundary rejection for the retired header, covered by the canonical HTTP malformed-header test.  The retained AL direct-peer client/listener remains the sole peer transport path; no application provenance protocol is restored. | Before/after header/route inventory; `python3 scripts/phase-am/check_legacy_transport_removal.py --category peer-ingress`; guard mutation test; focused runtime header-rejection test; `just test`; `just lint`. |
| AM1-RM-005 | Historical peer delivery coordinator, HTTPS transport, delivery projection, and peer observability files were already absent (`https_transport.rs`, `peer_drain_coordinator.rs`, `peer_delivery_observability.rs`). AM.5 removed the actual remaining replay state: `OutboundMessageQuery`, `StoredPeerWrite`, `SqliteOutboundMessageQuery`, its cursor/budget plumbing, and the serialized `peerOutbound.request` payload. `runtime_health/peer_delivery_router.rs` remains as the direct-send/synchronous-received-hook composition anchor; `post_commit_work.rs` remains explicitly no-background-work. | **AM.5 complete.** The retained `peerOutbound.host` is routing metadata only, not a replay payload. Retired `PeerSyncPolicy`, `max_message_age`, `max_batch_messages`, and `peer_sync_policies` have no live parser or config surface; the schema migration drops the old table so upgrades cannot retain it. Historical ADR/plan references remain evidence, not active configuration. | `python3 scripts/phase-am/check_legacy_transport_removal.py --category resend-replay`; mutation proof; direct-host no-hook integration accounting test; focused storage/core tests; `just lint`; `just test`. Never delete the retained hook path merely because its historical name mentions peer delivery. |
| AM1-RM-006 | `peer_resolution.rs`, `runtime_health/peer_authority.rs`, trusted-peer storage, and `atm-peer-tls-interop`/storage TLS types remain physical-address/trust candidates.  They are not sender replay workers. | Conditional retain; an owner must identify an actual live AL physical-adapter edge before any removal.  No TLS activation is authorized by AM.1. | Peer/TLS path search and M5 direct-host smoke.  Do not infer deletion from old HTTPS names. |
| AM1-RM-007 | Legacy tmux emitter in `atm-daemon/src/message_received_emitter.rs` is live; the Tokio replacement selector is also live in `atm-daemon-bootstrap`.  `atm-graft` is a separate supported boundary, not a daemon dependency to erase blindly. | Guard only prohibited daemon graft edges and any future legacy tmux adapter selected for deletion.  Exclude the current accepted legacy emitter until its own owner has a replacement and migration proof. | Guard mutation plus harness-specific tests; preserve current received-hook behavior. |
| AM1-RM-008 | Direct SQLite is absent from daemon and HTTP-runtime manifests/source.  Storage remains behind `atm-storage` / `atm-storage-rusqlite` boundaries; architecture enforcement forbids the bad edges. | Already clean; retain as an active negative category once a deletion PR enables it. | Direct-SQLite guard success and architecture boundary tests. |
| AM1-RM-009 | Raw transport tests/fixtures remain in API, daemon local IPC/TCP, daemon-client, architecture enforcement, and smoke support.  The AL11 subprocess test gap was waived because it would start the frozen legacy binary; the AL11 `process::exit` UDS-leak code defect is fixed. | Delete tests with the implementation row they specify; retain AL13/AL9 typed smoke and the AL11 lifecycle decision record. | Fixture search, focused replacement tests, then full test/lint. |

## Topology and retained boundaries

```text
atm / atm-graft -> atm-daemon-client -> shared typed HTTP client
legacy daemon local IPC/TCP -> removed by AM.3
canonical HTTP handler -> typed runtime request conversion -> defensive legacy-header rejection
durable write -> synchronous received-hook route -> supported tmux/graft selector
peer authority/DNS -> configured physical peer candidate (conditional retain)
```

The prior coordinator-to-HTTPS-to-replay chain is not a live topology row: its
implementation files are absent.  `peer_delivery_router` and
`post_commit_work` must instead be reviewed by their actual synchronous
received-hook behavior.  This avoids deleting current AL behavior based on
historical filenames.

`DaemonApiClient` is sealed under ADR-001.  AM.1 introduces no trait,
implementation, or crate-boundary change; any later client migration must
review the existing sealed implementations rather than adding an unauthorized
implementation.  The inventory also found no AM.1-owned newtype, lock, or
error surface requiring a Rust-pattern remediation (RBP-001/003/004/006).

## Draft negative guard and mutation proof

`scripts/phase-am/check_legacy_transport_removal.py` covers raw framing,
peer-only ingress, resend/replay, direct SQLite, and daemon harness edges.
Its tests are `.just/tests/test_phase_am_legacy_transport_guard.py`.

2026-08-10 evidence:

```text
python3 .just/tests/test_phase_am_legacy_transport_guard.py -v  # 14 passed
python3 scripts/phase-am/check_legacy_transport_removal.py --category direct-sqlite  # passed
```

The mutation tests prove each category fails for a reintroduced representative
symbol; direct-SQLite, raw-framing, peer-ingress, and resend/replay are empty
and enabled in `just lint`. The daemon-harness category remains intentionally
non-empty because RM-007 is live; it stays out of integration until its owner
makes it empty, enables it in the same PR, and retains its mutation test.

## Freeze and deletion rules

1. A future AM owner replaces the pending freeze task only with a concrete,
   accepted AL.9 live-reference graph SHA **and** its physical/benchmark
   evidence links.  A static graph or later-sprint artifact is insufficient.
2. No row is removed based solely on a phase number, an historical document, or
   an absent predecessor file.  Re-run the inventory, identify compiled callers,
   then delete caller before callee.
3. Delete implementation, Cargo edge, fixtures, docs, and selected negative
   guard in one owned PR; run focused tests, `just lint`, and `just test`.
4. Do not start, patch, or use frozen `atm-daemon` to prove AL runtime behavior.
   AL11's waived binary-level regression gap is tracked as a deletion-era
   decision, not authority to revive the legacy runtime.
