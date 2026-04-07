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
