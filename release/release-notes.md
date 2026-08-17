# Release Notes

## Summary

- version: 1.4.2
- release date: 2026-08-17
- release owner: publisher (ATM release execution)

Version 1.4.2 consolidates the caller-identity and post-send simplification
work, the Tokio/Axum HTTP runtime, deletion of the retired transport path,
decomposed template messages and query surface, and the first public-package
release lane for `hermes-atm` and `atm-graft`. It also records historical
transport lines honestly: Phase AG and Phase AK are retired/superseded rather
than features carried by this release.

## Included Changes

### Phase AD — caller identity and post-send simplification

- Restore direct post-send emission and remove retired Claude/reconciliation
  daemon paths.
- Converge the mailbox protocol on owner-only mutation, self-send rejection,
  and self-ack termination.

### Phase AG — retired cross-host design (historical context)

- Retain the early custom-frame/TCP cross-host design only as historical
  context. It was explicitly rejected and retired; it is not a v1.4.2
  transport feature.
- Phase AI, followed by the Phase AL/AM runtime, is the accepted replacement
  line.

### Phase AI — HTTP daemon and minimal cross-host transport

- Complete the initial HTTP transition through AI.52: HTTP over Unix-domain
  sockets on Unix and loopback TCP on Windows, graft bindings, per-profile
  launchd bridge deployment, peer authority/trust resolution, bounded peer
  recovery, reconciliation, reports, and fuzz tooling.
- The pre-AL/AM peer implementation is superseded by the Tokio runtime and
  deletion-only cleanup below. Physical two-Mac and Mac-to-Windows live peer
  proof remains a retained evidence gap, not a claim of completed coverage.

### Phase AK — superseded direct-peer design (historical context)

- The AK.1–AK.10 implementation line was superseded wholesale by the Phase
  AL/AM Tokio migration and has no independent release behavior.
- Its reviewed receiver-hook design was carried forward into AL.1; this is the
  only retained AK artifact used by the current runtime.

### Phase AJ — runtime observation (PR #759)

- Add daemon-resident session/PID heartbeat caching for diagnostic runtime
  observation.
- This telemetry is diagnostic only: it is not a routing, admission, or policy
  input.

### Phase AL — minimal Tokio HTTP runtime

- Ship `atm-http-runtime`, the maintained Tokio/Axum HTTP path used by the
  CLI, `atm-graft`, Unix UDS, loopback TCP, and direct peer delivery.
- Establish one typed request/response contract and canonical write ingress,
  with real M4-to-M5 direct peer send and acknowledgement evidence.

### Phase AM — deletion-only transport cleanup

- Remove handwritten HTTP framing, legacy local and peer transport workers,
  and redundant resend/replay machinery after proving each retired reference
  dead or recorded in the removal ledger.
- Preserve the explicit platform contract: Unix uses HTTP over UDS or
  loopback TCP; Windows uses HTTP over loopback TCP; peer delivery uses the
  same ordinary HTTP write schema.
- Retain transport-specific benchmark evidence, including M5 one-frame medians
  of approximately 15,012 UDS admissions/second and 12,340 loopback-TCP
  admissions/second, and a Windows one-frame TCP result of 16,175
  messages/second.
- Add `hermes-atm`, the first supported Hermes-harness integration. It installs
  in the Hermes Python environment, registers a profile-aware graft receiver,
  and supplies native `atm_send`, `atm_read`, and `atm_list` tool directions
  without requiring users to patch the Hermes harness source.

### Phase AN — decomposed template messages and query surface

- Add a durable decomposed template catalog, render-on-read, typed analyst
  search/query surface, and generic ATM compose workflow.
- Add optional template-declared workflow metadata, effective tags, and a
  generic OpenTelemetry-compatible lifecycle projection without embedding a
  specific team workflow in ATM.
- Adopt the exact released `sc-sha`, `sc-composer`, and `sc-compose` 1.4.1
  chain through the sealed `atm-template-sc-compose` adapter. Checked rendering
  rejects malformed rendered JSON before send, cache, or render-on-read output.
- Add adversarial assurance for realistic template and Tokio HTTP seams,
  including deterministic corpus/seed evidence and retained reports.

### PyPI and release packaging

- Add the first public-package lane for `hermes-atm` and the Maturin
  `atm-graft` package: CPython 3.11–3.14 compatibility, manylinux,
  musllinux, Windows, and aarch64 native wheels plus sdists, and the PyPI /
  TestPyPI publish workflow (PR #911).
- Align workspace and Python package versions under the one canonical
  `check_version_sync.py` gate, rather than the earlier duplicate checks
  (PRs #897 and #910).
- Complete PyPI metadata and secret-name readiness fixes (PRs #906, #907, and
  #914), and publish the Hermes ATM user installation/configuration guide
  (PR #916).
- Document the first-public-package versioning policy in ADR-049 (PR #909).

## Operator / User Impact

- The production daemon path is now the Tokio/Axum `atm-http-runtime` path;
  the retired synchronous/custom-framing transport is not a fallback.
- Local Windows use is supported through loopback TCP. Windows local tests,
  smoke coverage, and the FastPC4 TCP benchmark pass. The missing
  Windows-to-M4 live proof is an environment/VPN-DNS reachability limitation,
  not a claimed local transport failure.
- Hermes users can install the documented `hermes-atm` package into the Hermes
  Python environment and use profile-aware native tools and meaningful graft
  nudges through the existing agent session.
- Template-backed sends and render-on-read now have durable catalog identity,
  typed queryability, checked output validation, and reusable sc-compose
  integration. Installed documentation remains available at
  `share/doc/atm/`, with its entry point at `share/doc/atm/README.md`.

## Packaging / Distribution Notes

- crates.io: publish the 12 `publish = true` crates in the manifest's required
  dependency order:
  1. `atm-error`
  2. `atm-storage`
  3. `agent-team-mail-core`
  4. `atm-storage-rusqlite`
  5. `atm-http-runtime`
  6. `atm-daemon-client`
  7. `atm-runtime`
  8. `atm-template-sc-compose`
  9. `atm-daemon-bootstrap`
  10. `atm-daemon`
  11. `atm-graft`
  12. `agent-team-mail`
- `atm-error` and `atm-http-runtime` are new entries relative to the 1.3.1
  nine-crate manifest. `atm-template-sc-compose` was added after release
  preflight found `atm-daemon-bootstrap` depends on it at runtime (issue
  #923, fixed in PR #924) -- it was previously `publish = false` and absent
  from the manifest, which would have broken `cargo publish` mid-sequence.
  The Maturin `atm-graft-python` artifact remains in the release inventory
  but is published to PyPI as `atm-graft`, so it is not one of the 12
  crates.io artifacts.
- PyPI/TestPyPI: `hermes-atm` and `atm-graft` have a verified, installable
  TestPyPI milestone. The real PyPI publication is the release execution step,
  not a completed claim in these notes.
- GitHub Releases, Homebrew, and winget continue to be published by the
  release workflow after the ordered artifact gates pass.

## Known Issues / Waivers

- Physical two-Mac and Mac-to-Windows peer proof remains pending. The current
  Windows-to-M4 limitation is the available VPN/DNS topology, not an alternate
  or unsupported local transport path.
- Production peer TLS/mTLS is not part of v1.4.2. Quarantined TLS
  provisioning/storage material and curl mTLS interop fixtures are retained as
  reference material only; no TLS business logic is on the live daemon HTTP
  path.
- ADR-049 is merged but its own status is still **Proposed**. The release does
  not silently treat that status as Accepted; its final disposition remains a
  release-governance follow-up.

## Follow-Up

- Execute and retain the physical two-Mac and Mac-to-Windows peer matrix when
  the required network reachability is available.
- Implement the planned reusable `peer-tls` boundary in Phase AO, with
  certificate/key exchange behind a storage interface, TLS enabled after a
  successful exchange, and an explicit diagnostic/benchmark opt-out.
- Begin Phase AP by proving the outbound-initiated corporate-network approach
  against the current Windows VPN/DNS constraint before broadening its
  deployment scope.
