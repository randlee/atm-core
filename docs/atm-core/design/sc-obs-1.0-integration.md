# `sc-observability` 1.0 Integration Note

## 1. Purpose

This note defines the ATM-side follow-on work needed after Phase K so ATM can
adopt the final `sc-observability` 1.0 release cleanly.

Phase K proved that the current ATM adapter model works against the
pre-publish shared crates. Phase L is narrower:

- adopt the last shared usability/features needed by ATM (`#55`, `#57`, `#21`)
- depend on upstream shared consumer-doc cleanup from `#20` without creating a
  separate ATM implementation sprint just for shared documentation work
- keep all pre-publish implementation sprints on the local
  `[patch.crates-io]` strategy
- switch to crates.io `^1.0.0` only after the shared 1.0 release exists

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

## 4. Fault Injection Strategy (`#57`)

Phase K live validation proved the healthy real-adapter path only. Degraded and
unavailable states remained covered by deterministic ATM tests because the
shared public API did not provide a safe live failure trigger.

Required Phase L direction:

- use the upstream public fault-injection capability from `#57`
- drive degraded and unavailable retained-sink states through the real ATM
  adapter, not only through ATM-local doubles
- extend live validation so all three states are exercised:
  - healthy
  - degraded
  - unavailable

Expected implementation shape:

- keep deterministic ATM integration tests as the fast regression layer
- add real-adapter end-to-end tests or scripted validation that use the shared
  fault injection API directly through the `atm` adapter
- update the live validation report with the exact degraded/unavailable
  scenarios exercised

This closes the pre-publish readiness gap that remained after Phase K.

## 5. File Sink Path Alignment (`#21`)

ATM must not hardcode assumptions about the earlier retained file layout once
the shared crate adopts the newer path shape.

Required Phase L direction:

- ATM-side path assumptions must follow the shared crate layout
- retained query/follow and doctor health checks must be revalidated against
  the new location
- operator-facing docs and validation notes must use the current shared layout

## 6. Dependency Strategy

ATM now consumes the published `sc-observability = "^1.0.0"` release. Phase L
continues from that published baseline rather than the earlier local
`[patch.crates-io]` pre-publish strategy.
