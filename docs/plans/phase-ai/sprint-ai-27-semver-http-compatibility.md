---
title: AI.27 SemVer release and HTTP compatibility
status: proposed
branch: feature/pAI-s27-semver-http-compatibility
target: integrate/phase-AI
depends_on: AI.11–AI.16
parallel_with: AI.22–AI.25
blocks: AI.26
---

# AI.27 — SemVer release and HTTP compatibility

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

## Required signatures

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

`ReleaseVersion` and `HttpApiVersion` parse strict SemVer. The compatibility
verdict returns both daemon versions for diagnostics. It must not compare
product release strings for admission.

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
