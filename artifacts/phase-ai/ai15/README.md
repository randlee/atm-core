# AI.15 evidence (non-physical)

This directory holds AI.15 release-readiness evidence captured on a single
physical macOS host. Every file carries an explicit `evidence_class` marker and
`physical_peer_pair` / `physical_windows_hardware` booleans so the class of each
artifact is unambiguous (RBQA-F005).

None of this is physical Mac<->Windows peer-pair evidence. Physical execution
across a real macOS host and a real Windows host remains open and is tracked by
`AI10-WINDOWS-001` and `AI14-QA1-EVIDENCE-GAP`. The sprint document keeps
`status: proposed` until that physical evidence is attached.

## Files

| File | `evidence_class` | What it is |
| --- | --- | --- |
| `mac-release-build.json` | `physical-macos-build` | Real macOS release build of the same commit; the migrated loopback-TCP graft transport links into the `atm` binary. Not a peer-pair result. |
| `https-cert-trust-config-validation.json` | `loopback-local` | Loopback-only validation of the graft capability-authenticated control plane and a reference to the single-host peer_config cert/trust shape. Not reciprocal two-host trust. |
| `ai13-runner-mock-exercise.json` | `mock-runner` | AI.13 peer-pair runner exercised in `--validate-only` (mock) mode with placeholder loopback commands. No network send/read/nudge/ack. Not ATM peer-pair evidence. |

## `evidence_class` values

- `physical-macos-build` — real work performed on this physical macOS host, but
  not a cross-host peer-pair case.
- `loopback-local` — same-host `127.0.0.1` exercise only.
- `mock-runner` — the runner mechanics were validated with mock commands; no
  real transport was exercised.

A future `physical-peer-pair` class is reserved for the two-host run that closes
`AI10-WINDOWS-001` / `AI14-QA1-EVIDENCE-GAP`; no file in this directory carries
that class.
