---
title: AO.5 — Real-host mTLS and TCP benchmark proof
status: planned
recommended_agent: arch-ctm
branch: feature/pao-s5-platform-benchmark-proof
worktree: ../atm-core-worktrees/feature/pao-s5-platform-benchmark-proof
---

# AO.5 — Real-host mTLS and TCP benchmark proof

## Scope

Run and retain the hardware-dependent Phase AO evidence after AO.3's canonical
mTLS behavior and AO.4's two benchmark modes have merged. AO.5 owns the M4↔M5
two-host mTLS proof and the TCP/plaintext plus TCP/mTLS benchmark comparison on
each available target: M4, M5, and FastPC4 (Windows). It is the sole owner of
real-host availability; AO.1–AO.4 close without waiting for those machines.

## Dependencies

- **must_follow:** AO.3 and AO.4 PRs merged. Merge their integration result
  before every AO.5 development/fix/evidence round so every retained result
  names the tested immutable candidate SHA.
- **parallel_safe:** none. This sprint owns the shared managed-daemon and
  report-index evidence artifacts.
- **unblocks:** Phase AO physical-performance exit and release comparison.

## Deliverables

1. A controlled benchmark procedure for every target: select one matching
   release-candidate daemon/CLI pair; switch to a disposable ATM database;
   run; restore the original database; then leave the release candidate
   installed for dogfooding unless a verified code bug or material performance
   regression requires restoring the prior pair.
2. On M4 and M5, bidirectional canonical mTLS send/read/requires-ack/reply,
   direct mTLS preflight, and retained safe smoke reports. Host identity in
   records is the stable hostname, never a transient IP address.
3. On M4, M5, and FastPC4, run `just benchmark tcp` and
   `just benchmark tcp-tls`; publish both mode-labeled artifacts with source
   SHA, platform, hardware label, hook mode, and host-state-isolation result.
   FastPC4's local TCP evidence is required even when its VPN prevents the
   separate cross-host row.
4. A mode-aware performance comparison against the latest compatible baseline.
   A slower mTLS result is not automatically a failure; a reproducible
   material regression against its own mode's baseline must be investigated,
   re-run, and either fixed or retained with an explicit root-cause record.
5. One report-index update linking every passing/failed retained AO.5 artifact
   without private keys, certificate bundles, database paths, or IP-derived
   identity claims.

## Acceptance criteria

- Every proof target runs matching daemon and CLI versions from the recorded
  candidate SHA, and doctor reports the selected direct-peer wire mode.
- mTLS proof never uses the frozen daemon, a legacy IPC path, or a plaintext
  retry. A TCP/plaintext run is visibly labeled as the benchmark diagnostic
  profile and never satisfies mTLS acceptance.
- Each benchmark profile compares only with the same host, TCP transport,
  frame profile, hook mode, and `peer_wire_security` mode.
- Database and managed-daemon restoration are verified even on a failed run;
  dogfooding follows the candidate only after no verified functional defect or
  material regression is found.
- An unavailable M5 or FastPC4 produces an honest blocked evidence row, not a
  substituted local result or a false completion claim.

## Required validation

- Before every physical run: candidate build, `atm doctor --json`, and local
  smoke on that host.
- M4/M5: direct mTLS preflight plus both directions of send/read/ACK/reply.
- Each available target: `just benchmark tcp`, `just benchmark tcp-tls`,
  `just benchmark-report`, report-index validation, and post-run daemon/CLI
  version plus database-restoration verification.
- `just lint` and `just test` at the candidate SHA that owns any script,
  evidence-schema, or report-index change.

## Non-closure

AO.5 does not prove Windows VPN/corporate-firewall connectivity; Phase AP owns
outbound-initiated reachability. It does not change TLS policy, key exchange,
message semantics, or the benchmark mode contract established by AO.4.
