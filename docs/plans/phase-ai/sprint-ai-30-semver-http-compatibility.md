---
title: AI.30 SemVer release and HTTP compatibility
status: proposed
branch: feature/pAI-s30-semver-http-compatibility
target: integrate/phase-AI
depends_on: AI.11–AI.16
parallel_with: AI.25–AI.28
blocks: AI.29
---

# AI.30 — SemVer release and HTTP compatibility

## Release candidate

- First commit: set every releasable ATM assembly to `1.3.2-beta-30`; record
  matching client/daemon values from `atm doctor --json` in runtime evidence.

## Closure

Release labels cannot block an otherwise compatible CLI and daemon. The HTTP
contract has a strict SemVer identity, and same-major additive changes remain
interoperable.

## Deliverables

1. Replace release-equality admission with the explicit schema and HTTP API
   compatibility contract from ADR-027/033/042.
2. Define and expose strict SemVer product release, CLI/daemon schema, and
   HTTP API version values independently through compatibility and doctor.
3. Make the OpenAPI contract and route implementation accept additive request
   fields, tolerate additive response fields, and reject only an incompatible
   HTTP major or schema before a write.
4. Add opt-in prerelease publication support for `alpha`/`beta` SemVer builds
   through `atm-beta`; keep normal Homebrew `atm` stable-only.

## Implementation map

- `crates/atm-core/src/protocol.rs`: replace release-string admission with
  strict `ReleaseVersion`, schema-version, and `HttpApiVersion` DTO fields.
- `crates/atm-daemon-client/src/compatibility.rs` and
  `crates/atm-daemon/src/runtime_health.rs`: exchange and decide the same
  compatibility preflight/verdict; neither independently compares releases.
- `crates/atm-core/src/api.rs` and the OpenAPI source: declare `/v{major}` and
  additive-field decoding once for local and peer HTTP adapters.
- repo release/Homebrew scripts: publish explicit `atm-beta` prereleases only;
  normal stable formula selection cannot resolve a prerelease.

## Required signatures

Current state on `origin/integrate/phase-AI@cb3af95188c1ba685ed93cec0512e7d38fa7f655`:

```rust
// Migrated previously from api.rs to protocol.rs; this sprint keeps protocol.rs
// as its sole owner.
pub struct CompatibilityPreflight {
    pub client_release: ReleaseVersion,
    pub wire_version: u16,
}

pub enum CompatibilityVerdict {
    Compatible { daemon_release: ReleaseVersion },
    Incompatible {
        client_release: ReleaseVersion,
        daemon_release: ReleaseVersion,
        code: AtmErrorCode,
    },
}
```

Target state: evolve (do not add a second preflight/verdict) the existing
`protocol.rs` types. `wire_version` is renamed to `cli_schema_version`; the
new `http_api_version` field is added. `client_release` remains diagnostic
only. `CompatibilityVerdict::Incompatible.code` is retained and carries the
typed schema- or HTTP-major mismatch code; no release-string mismatch is an
admission failure.

```rust
pub struct CompatibilityPreflight {
    pub client_release: ReleaseVersion,
    pub cli_schema_version: u16,
    pub http_api_version: HttpApiVersion,
}

pub struct HttpApiVersion(semver::Version);
// Admission: cli_schema_version == daemon_schema_version
//         && http_api_version.major == daemon_http_api_version.major
```

`ReleaseVersion` and `HttpApiVersion` parse strict SemVer. The evolved
compatibility verdict returns both daemon versions for diagnostics and retains
its typed `code`. It must not compare product release strings for admission.

## Acceptance criteria

- `1.3.1` CLI and `1.3.2-beta.1` daemon dispatch an existing write when their
  schema and HTTP API major match.
- A schema mismatch and an HTTP-major mismatch each reject before canonical
  write/persistence, with a typed error naming both values.
- A client/server minor or patch difference completes every common existing
  endpoint; omitted additive fields default and unknown additive fields do not
  fail decoding.
- Strict SemVer accepts `1.3.2-alpha.1` and `1.3.2-beta.1`, rejects malformed
  identifiers, and doctor reports prerelease labels unchanged.
- Normal Homebrew `atm` remains stable-only; `atm-beta` is explicitly opt-in.

## Required validation

`just lint`; `just test`; OpenAPI schema validation; compatibility matrix tests
for release mismatch, schema mismatch, HTTP major mismatch, and same-major
minor/patch interoperability; Homebrew formula selection test or deterministic
release-script fixture.

## Non-closure

This sprint does not introduce a new public endpoint, negotiate arbitrary
feature sets, or loosen TLS/peer authorization. It changes only compatibility
admission and prerelease distribution selection.
