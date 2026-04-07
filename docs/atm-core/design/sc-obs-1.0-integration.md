# `sc-observability` 1.0 Integration Note

## 1. Purpose

This note defines the ATM-side follow-on work needed after Phase K so ATM can
adopt the final `sc-observability` 1.0 release cleanly.

Phase K proved that the current ATM adapter model works against the
pre-publish shared crates. Phase L is narrower:

- adopt the last shared usability/features needed by ATM (`#55`, `#57`, `#21`)
- depend on upstream shared consumer-doc cleanup from `#20` without creating a
  separate ATM implementation sprint just for shared documentation work
- finish the remaining release-alignment work against the published
  `sc-observability = "1.0.0"` release

## 2. Scope

Phase L is ATM-side integration work only.

It does not redefine:

- the ATM-owned `ObservabilityPort` boundary
- ATM-owned error codes
- the `atm-core` rule that shared crate types must not leak through public ATM
  APIs

It refines the `atm` adapter and retained-command validation against the final
shared 1.0 crate behavior.

## 3. Stderr Routing Strategy (`#55`)

`L.1` is complete.

ATM now keeps retained console logging disabled by default and exposes one
explicit CLI routing switch:

- `--stderr-logs`

When present, the `atm` crate wires `ConsoleSink::stderr()` into the shared
logger builder. When absent, ATM leaves the shared console sink disabled so
normal command stdout output remains unchanged.

Rationale:

- the routing decision stays entirely inside the `atm` crate
- `atm-core` still exposes only the ATM-owned `ObservabilityPort` boundary
- stderr is opt-in, so scripted stdout consumers keep the same behavior unless
  they explicitly request retained console output
- the switch is straightforward to test in integration coverage

Implementation notes:

- `CliObservability` now uses `Logger::builder(...)` so the CLI adapter can
  register `ConsoleSink::stderr()` without changing `atm-core`
- stdout command rendering remains owned by ATM output code
- integration tests cover both:
  - default mode: stdout command output stays clean and stderr remains empty
  - `--stderr-logs`: retained console output is emitted on stderr

This change is useful for:

- CLI usability
- tests that need to distinguish retained log output from normal command output
- reducing accidental stdout contamination in scripted ATM usage

Traceability:

- the final CLI flag name is `--stderr-logs`
- this section is the canonical L.1 reference for that flag and closes the
  earlier traceability gap that QA tracked as `ATM-QA-002`

## 4. Fault Injection Strategy (`#57`)

`L.2` adopts the upstream retained-sink fault injector from `#57`.

Implemented shape:

- ATM keeps deterministic integration tests as the fast regression layer
- ATM also exposes one validation-only env seam:
  - `ATM_OBSERVABILITY_RETAINED_SINK_FAULT=degraded|unavailable`
- that seam still uses the real shared `RetainedSinkFaultInjector` through the
  `atm` adapter rather than ATM-local health doubles
- live validation now exercises all three states:
  - healthy
  - degraded
  - unavailable

Current outcome:

- end-to-end `atm doctor` coverage now verifies degraded and unavailable states
  through the real shared adapter path
- the live validation report records the induced degraded/unavailable runs
  explicitly
- the earlier healthy-only validation gap from Phase K is closed
- the ATM-owned observability health contract remains intentionally closed for
  initial release:
  - `healthy`
  - `degraded`
  - `unavailable`
- additional health nuance is deferred until a versioned follow-on change has
  a concrete consumer need

## 5. File Sink Path Alignment (`#21`)

ATM must not hardcode assumptions about the earlier retained file layout once
the shared crate adopts the newer path shape.

`L.3` closes the path-migration work by treating the shared adapter as the
source of truth for the active retained log file.

Current rule:

- ATM-side path assumptions must follow the shared crate layout
- the retained file sink now lives at:
  - `$ATM_HOME/.local/share/logs/<service_name>.log.jsonl`
- for ATM itself, the operator-facing expected path is:
  - `$ATM_HOME/.local/share/logs/atm.log.jsonl`
- retained query/follow and doctor health checks must be revalidated against
  the new location
- operator-facing docs and validation notes must use the current shared layout

### L.3 Carry-In Closure

Phase L.3 closes the retained Phase K carry-ins that belonged to the file-sink
alignment follow-up:

- `RUST-QA-001`
  - resolved by documenting the intended ATM-local ownership split rather than
    promoting the full concrete query/follow adapter surface into `atm-core`
  - `atm-core` remains the owner of the ATM-facing observability contract
    needed by ATM messaging workflows, while `atm` owns the concrete adapter
    wiring
- `PRR-002`
  - resolved by explicitly keeping the ATM observability health contract
    closed for initial release:
    - `healthy`
    - `degraded`
    - `unavailable`
- `ATM-QA-002`
  - resolved by treating the final `--stderr-logs` flag contract in this note
    as the canonical L.1 reference

Phase L.3 does not implement any L.7 config fields in code. Any merge-forward
from later Phase L.7 planning adds documentation for `[atm].team_members`,
`[atm].aliases`, and `[atm].post_send_hook` only.

## 6. Dependency Strategy

ATM now consumes the published `sc-observability = "1.0.0"` release. Phase L
continues from that published baseline rather than the earlier local
`[patch.crates-io]` pre-publish strategy.

## 7. L.4 Public Boundary Cleanup

L.4 is a Rust API cleanup sprint, not a CLI JSON redesign.

Implementation contract:
- replace public raw `serde_json::Value` / `serde_json::Map` usage with the
  ATM-owned field model:
  - `LogFieldKey`
  - `AtmJsonNumber`
  - `LogFieldValue`
  - `LogFieldMap`
- update `LogFieldMatch` and `AtmLogRecord.fields` to use that field model
- keep all raw `serde_json` parsing and adapter translation inside `atm-core`
- preserve the current CLI JSON output shape for retained-log commands
- `AgentMember.extra` remains explicitly out of scope for L.4 because it uses
  `#[serde(flatten)]` for round-trip preservation of Claude Code fields rather
  than the observability-facing field model

`AtmJsonNumber` contract:
- `AtmJsonNumber` accepts any valid JSON number allowed by RFC 8259
- `AtmJsonNumber` must reject non-JSON numeric values such as `NaN`,
  `Infinity`, and `-Infinity`
- its constructor returns `Result<AtmJsonNumber, AtmError>`

Closes:
- `INTEROP-001`
- `BP-003`

## 8. L.5 Construction Ergonomics

L.5 is a construction-contract cleanup sprint.

Implementation contract:
- add `CliObservability::new(home_dir, CliObservabilityOptions)`
- keep `init(...)` only as a delegating CLI bootstrap helper
- keep dynamic dispatch and the current sealed-trait pattern unless
  implementation surfaces a concrete defect
- keep `DoctorCommand` injectability deferred for initial release unless a
  concrete need appears during implementation

Closes:
- `UX-001`
- `BP-004`

Dispositions:
- `UX-002`: retained
  - dynamic dispatch via `Box<dyn ObservabilityPort + Send + Sync>` remains
    acceptable for initial release because it keeps the CLI bootstrap surface
    simple without weakening the ATM-owned boundary
- `BP-001`: retained
  - the current sealed-trait pattern remains in place because it prevents
    arbitrary external `ObservabilityPort` implementations from bypassing the
    intended ATM adapter boundary; this should be revisited only if an
    alternative clearly reduces construction complexity or materially improves
    first-party testing without weakening crate-boundary guarantees
- `UNI-003`: deferred
  - `DoctorCommand` injectability remains out of scope for initial release
    unless implementation exposes a concrete testability or composition need

## 9. L.6 Release Closeout

L.6 closes the remaining release-critical operator-facing carry-forward items
without reopening the broader `.atm.toml` planning note from `L.7`.

Closes:
- `ATM-QA-001`
  - runtime identity no longer falls back to obsolete `.atm.toml`
    `[atm].identity`; the retained multi-agent model now requires runtime
    identity to come from explicit command override when supported, hook
    identity, or `ATM_IDENTITY`
- `ATM-QA-002`
  - `atm doctor` now reports obsolete `[atm].identity` as
    `ATM_WARNING_IDENTITY_DRIFT` and directs operators to remove the field and
    set `ATM_IDENTITY` in the active environment instead

Validation closeout:
- the published `sc-observability = "1.0.0"` baseline was revalidated after
  `L.4`, `L.5`, and `L.6`
- healthy, degraded, and unavailable doctor states still map to the expected
  shared-adapter health contract
- the drift warning is additive: it does not replace the observability health
  finding and therefore surfaces as a warning alongside healthy observability
  when obsolete config identity is still present
