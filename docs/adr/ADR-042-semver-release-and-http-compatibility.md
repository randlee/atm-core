# ADR-042 — SemVer Release And HTTP Compatibility

| Field | Value |
| --- | --- |
| ID | ADR-042 |
| Status | Proposed |
| Relates to | ADR-027, ADR-033, Phase AI.27 |

## Decision

ATM distinguishes three independent values:

1. **Product release SemVer** identifies a build and accepts strict SemVer,
   including prereleases such as `1.3.2-beta.1`.
2. **CLI/daemon schema version** identifies the local compatibility-preflight
   shape and semantics.
3. **HTTP API SemVer** identifies the REST contract. Its major is the path
   major in `/v{major}/atm`.

Only the schema version and HTTP API major may reject a write before dispatch.
Product release mismatches never do. Same-major HTTP additions are compatible:
new fields are optional/defaulted, unknown additive fields are ignored, and
new response fields are tolerated. A new operation declares a capability
requirement instead of turning a minor-version difference into a global
connection failure.

Stable releases publish through the normal Homebrew `atm` formula. Approved
prerelease artifacts publish only through the opt-in `atm-beta` formula in the
project-owned tap; they never replace the stable formula's default version.

## Consequences

- Developers can run a newer daemon with an older compatible CLI while
  debugging or performing a controlled rollout.
- A breaking HTTP change requires a new `/v{major}` route family and explicit
  migration; it cannot hide behind a product patch release.
- The compatibility response and `atm doctor` disclose product, schema, and
  HTTP API versions independently.
