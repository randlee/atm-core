---
title: AO.4 — Cross-platform TCP mTLS benchmark modes
status: planned
recommended_agent: arch-ctm
branch: feature/pao-s4-tls-benchmark-modes
worktree: ../atm-core-worktrees/feature/pao-s4-tls-benchmark-modes
---

# AO.4 — Cross-platform TCP mTLS benchmark modes

## Scope

Make the isolated benchmark harness measure the canonical direct-peer TCP
route in two explicit launch modes on macOS, Linux, and Windows: the existing
untrusted plaintext diagnostic profile and normal mutual TLS. The same
release-built benchmark binary must select either mode at process launch; a
mode change must not require recompiling crates, changing application request
logic, or using the frozen `crates/atm-daemon` tree.

The user-facing targets are:

```text
just benchmark tcp       # direct-peer TCP, explicit plaintext-test profile
just benchmark tcp-tls   # direct-peer TCP, mutual-TLS profile
```

Internally, both artifacts retain `transport: "tcp"` and add a distinct
`peer_wire_security` field. `tcp-tls` is a recipe target, not a misleading
third wire transport; this keeps TLS overhead comparison explicit and prevents
a TLS result from being selected as a plaintext baseline (or vice versa).

## Dependencies

- **must_follow:** AO.3 PR merged. AO.4 measures the immutable canonical mTLS
  path that AO.3 proves; it must merge AO.3 before every development/fix round.
- **parallel_safe:** AO.5 physical platform proof is parallel-safe after this
  sprint's benchmark contract is pushed, because it consumes only the stable
  command and evidence schema without changing them.
- **unblocks:** AO.5 retained M4/M5/FastPC4 performance evidence.

## Deliverables

1. A benchmark-harness-only, typed launch selection with exactly two modes:

   ```rust
   enum BenchmarkPeerWireSecurity {
       PlaintextTest,
       MutualTls,
   }
   ```

   The benchmark child receives this selection explicitly. `PlaintextTest`
   constructs the existing named `DirectPeerPlaintextDiagnostic::Benchmark`;
   `MutualTls` constructs the existing sealed `PeerIoAdapter` from disposable
   benchmark peer configuration. Configuration, DNS, certificate, hostname,
   pin, or handshake failures fail the mTLS run and never select plaintext.

2. Disposable benchmark identity/trust setup for the mTLS mode, including a
   localhost-compatible certificate, enabled interface, and exact local peer
   trust record. It uses normal storage and the `peer-tls` adapter; it neither
   imports Rustls into `atm-http-runtime` nor adds a second HTTP handler or
   client path.

3. `just benchmark` target parsing that accepts `tcp` and `tcp-tls` on every
   supported platform, passes the corresponding explicit mode to the runner,
   and preserves the existing UDS/default benchmark behavior. Windows must
   reject UDS as it does today and accept both TCP targets.

4. Evidence/schema/report changes that record
   `peer_wire_security: "plaintext_test" | "mutual_tls"`, include it in the
   artifact name and rendered report label, and key baseline/comparison lookup
   by both TCP transport and wire-security mode. The public artifact contains
   no private key, certificate bundle, or database path.

5. Focused cross-platform tests for target parsing, invalid/implicit mode
   rejection, plaintext diagnostic labeling, successful disposable mTLS setup,
   rejection-before-HTTP-dispatch for an invalid mTLS peer, mode-aware baseline
   selection, JSON schema validation, and report rendering.

## Acceptance criteria

- `just benchmark tcp` and `just benchmark tcp-tls` exercise the same direct
  peer HTTP request/result path and differ only in the explicitly recorded
  stream-security mode.
- A single built benchmark executable can run both modes by changing its
  launch argument; release/runtime crates are not recompiled for the switch.
- mTLS is normal adapter behavior and plaintext is the named benchmark-only
  diagnostic override; no TLS failure can downgrade the run.
- TCP mTLS runs are supported on macOS, Linux, and Windows with no Unix-socket
  dependency or platform-specific source fork.
- Artifact comparison refuses cross-mode baselines, and generated JSON/XHTML
  identifies the mode and source revision.
- `atm-http-runtime` remains opaque to TLS configuration and types, and the
  frozen daemon is neither changed nor executed.

## Required validation

- Focused Rust tests for benchmark-mode argument parsing and bootstrap
  composition; `cargo test -p atm-daemon-bootstrap -p atm-http-runtime -p peer-tls`.
- Python tests for benchmark target parsing, schema, baseline matching, and
  report rendering on simulated Unix and Windows platform selections.
- One disposable local `tcp` and `tcp-tls` run on the implementation host;
  assert distinct valid artifacts rather than a throughput threshold between
  the modes.
- `just lint` and `just test`.

## Non-closure

AO.4 defines and validates the benchmark modes; it does not claim retained
M4↔M5, Windows/FastPC4, or release-candidate performance results. AO.5 owns
that physical proof and may defer any AO.1–AO.3 host-dependent evidence there.
