# `sc-observability` 1.0 Integration Note

## 1. Purpose

This note defines the ATM-side follow-on work needed after Phase K so ATM can
adopt the final `sc-observability` 1.0 release cleanly.

Phase K proved the ATM adapter model and completed the crates.io cutover.
Phase L is the release-hardening phase that:

- adopts the remaining shared usability/features ATM needs (`#55`, `#57`,
  `#21`)
- closes the remaining validation and public-boundary cleanup items before
  initial release
- keeps ATM focused on messaging workflows rather than pre-owning future
  hook/`schooks` orchestration

## 2. Scope

Phase L is ATM-side release-hardening work only.

It does not redefine:

- the ATM-local `ObservabilityPort` boundary needed by retained ATM messaging,
  `atm log`, and `atm doctor`
- ATM-owned error codes
- the `atm-core` rule that shared crate types and raw `serde_json` value types
  must not leak through public ATM APIs

It refines the `atm` adapter, retained-command validation, and public API
cleanup against the final shared 1.0 crate behavior.

## 3. Stderr Routing Strategy (`#55`)

Current ATM behavior uses the shared console sink conservatively because normal
CLI command output must remain under ATM control.

Required Phase L direction:

- `CliObservability` should support a retained-log console route that targets
  stderr through `ConsoleSink::stderr()`
- ATM must preserve the distinction between:
  - normal command output rendered by ATM CLI code
  - retained/shared console log output emitted by the observability adapter
- stderr routing must not leak shared sink behavior into `atm-core`; it remains
  an `atm` adapter concern

Expected implementation shape:

- add a CLI-facing selection rule through the explicit global flag
  `--stderr-logs`
- keep stdout behavior unchanged unless the routing rule selects stderr
- verify both output paths in integration tests

This change is useful for:

- CLI usability
- tests that need to distinguish retained log output from normal command output
- reducing accidental stdout contamination in scripted ATM usage

Implementation status:

- complete in Phase `L.1`
- shipped ATM-facing flag name: `--stderr-logs`
- the final flag name is intentionally documented here to close the L.1 QA
  traceability finding `ATM-QA-002`

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

This closes the healthy-only live-validation gap that remained after Phase K.

Phase-L release-hardening addition:

- `L.2` also closes the retained command-emission coverage gap by proving
  `send`, `read`, `ack`, and `clear` each emit retained records through the
  shared adapter

## 5. File Sink Path Alignment (`#21`)

ATM must not hardcode assumptions about the earlier retained file layout once
the shared crate adopts the newer path shape.

Required Phase L direction:

- ATM-side path assumptions must follow the shared crate layout
- retained query/follow and doctor health checks must be revalidated against
  the new location
- operator-facing docs and validation notes must use the current shared layout

Phase-L release-hardening addition:

- file sink path alignment is part of final release closeout rather than a
  standalone ownership refactor

## 6. Initial-Release Boundary Rulings

Phase L uses these architectural decisions:

- ATM observability remains scoped to ATM’s messaging workflows, retained-log
  query/follow, and doctor readiness checks
- future hook- or `schooks`-driven observability orchestration is explicitly
  out of scope for the initial release
- the health contract remains intentionally closed at:
  - `Healthy`
  - `Degraded`
  - `Unavailable`
- public `atm-core` observability APIs must not expose raw
  `serde_json::Value` / `Map<String, Value>` directly
- JSON/JSONL parsing, validation, degradation, and repair remain centralized in
  `atm-core` even after the public API cleanup

## 7. Dependency Strategy

Phase L uses the published crates.io dependency directly:

- `sc-observability = "1.0.0"`

No local `[patch.crates-io]` strategy remains in scope for this phase.

## 8. Remaining Phase L Sprint Mapping

- `L.1` complete:
  - stderr routing through `ConsoleSink::stderr()` and `--stderr-logs`
- `L.2`:
  - fault-injected degraded/unavailable live validation
  - retained-log emission integration coverage for `send`, `read`, `ack`,
    `clear`
- `L.3`:
  - ATM-local boundary wording and closed-health-contract rulings
- `L.4`:
  - public `serde_json` leakage cleanup at the `atm-core` observability
    boundary
- `L.5`:
  - structured `CliObservability` construction and remaining boundary
    ergonomics/disposition work
- `L.6`:
  - file sink path release closeout and final validation/signoff
