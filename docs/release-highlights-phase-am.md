# Phase AM Release Highlights

This note records the significant user-visible and operator-visible work in
the Phase AL/AM release line. It is intentionally a release summary rather
than a substitute for the sprint evidence or the release-preflight checklist.

## What ships

### One Tokio/Axum HTTP daemon path

ATM now uses the maintained `atm-http-runtime` for its production transport
path. The CLI, `atm-graft`, Unix UDS, loopback TCP, and direct peer delivery
use one typed HTTP request/response contract and one canonical write ingress.
Phase AM removes the redundant handwritten framing and legacy compatibility
transport machinery instead of retaining a fallback protocol.

This simplifies failure handling and makes the operating-system split
explicit: Unix uses HTTP over UDS or loopback TCP; Windows uses HTTP over
loopback TCP; peer delivery uses the same ordinary HTTP write schema.

### Measured durable throughput

The release includes retained benchmark artifacts for direct SQLite admission,
public UDS, and public loopback TCP. The M5 hardware results include a median
of approximately 15,012 UDS admissions/second and 12,340 loopback-TCP
admissions/second in the one-frame profile, with durable restart verification.
The FastPC4 Windows six-profile TCP campaign passed, with its one-frame result
at 16,175 messages/second and the raw JSON/XHTML artifacts retained under
[`site/reports/send-message-benchmark/`](../site/reports/send-message-benchmark/).

The report set is deliberately transport-specific; it does not hide UDS,
loopback TCP, or storage-admission costs behind one aggregate number.

### First Hermes harness integration

`hermes-atm` is the first supported ATM package for a live Hermes harness. It
is installable into the Hermes Python environment, registers a profile-aware
graft receiver, and delivers a meaningful ATM nudge through the existing
Telegram session and agent loop. The integration supports the native tool
direction (`atm_send`, `atm_read`, and `atm_list`) rather than requiring agents to shell
out for ordinary tool calls.

The release closure includes the Hermes package/PyPI lane as a Phase AM
prerequisite, alongside the M5 CPython 3.11 validation lane. A profile is
configured through installation and environment/profile data; a consumer does
not need to patch the Hermes harness source to use it.

### Reusable sc-compose integration

ATM's confined template rendering uses the reusable `sc-composer` library
through `atm-template-sc-compose`. This puts the template boundary on a
separately versioned composition library and makes its LF-normalized
content/graph hashing (`sc-sha`) available to ATM without copying renderer
logic into the daemon.

The matching `sc-compose` 1.4.0 release is prepared with an ordered
`sc-sha` -> `sc-composer` -> `sc-compose` crates.io publication pipeline,
both Python distributions, and CI-enforced Rust/Python version lockstep. Its
actual publish remains coordinated with the overall release close-out.

## Platform and cross-host status

M4 <-> M5 direct peer HTTP send and acknowledgement proof has passed. The
same canonical HTTP route, response reserve, acknowledgement routing, daemon
switch, and recovery behavior were exercised on real hardware.

Windows is a supported local daemon and cross-host implementation target:
the Windows test suite, local smoke, and FastPC4 TCP benchmark campaign pass.
The absent Windows <-> M4 live proof is an environment-limited evidence gap,
not a Windows or ATM transport failure. On the available CWin network,
`rand-m5.local`/M4 resolution and routing are unavailable through the current
VPN/DNS topology. With normal reachability, the same Windows loopback-TCP and
peer HTTP path is expected to work cross-host. Phase AP starts by proving an
outbound-initiated approach against this exact corporate-network constraint.

## TLS status

Production TLS/mTLS peer transport is **not** part of this release. Existing
TLS provisioning/storage types and the curl mTLS interop fixture are retained
as quarantined reference material; they are not on the live Phase AM peer
path. Phase AO is the next planned implementation: a reusable `peer-tls`
boundary with certificate/key exchange behind a storage interface, TLS enabled
by default once exchange succeeds, and an explicit off switch for benchmark
and diagnostic runs. No TLS business logic is to leak into the daemon's HTTP
path.

## Release gate

Before a `develop` -> `main` release merge, Phase AM must be present in
`develop`, including the `hermes-atm` package/PyPI release evidence. The
sc-compose 1.4.0 publication is likewise deferred until the full release
close-out is approved.
