# ATM Crate Requirements

## 1. Purpose

This document defines the `atm` crate requirements.

The `atm` crate owns the CLI layer and the CLI-side daemon client only.
Product behavior remains defined in [`../requirements.md`](../requirements.md).
`atm` must satisfy those product requirements without re-owning `atm-core`
business logic or `atm-daemon` runtime behavior.

## 2. Ownership

`atm` owns:

- clap command parsing
- command dispatch
- user-facing output rendering
- process exit behavior
- one-time observability bootstrap
- concrete implementation of the `atm-core` observability boundary
- CLI-side request mapping into the daemon/service API

`atm` does not own:

- mailbox mutation logic
- state-machine logic
- config resolution policy
- log query business logic
- doctor business logic
- singleton daemon lifecycle
- direct SQLite access
- direct inbox JSONL parsing or writes

## 3. Requirement Namespace

The `atm` crate uses the `REQ-ATM-*` namespace.

Initial allocation:

- `REQ-ATM-CMD-*` for command-entry requirements
- `REQ-ATM-OUT-*` for output/rendering requirements
- `REQ-ATM-OBS-*` for observability-bootstrap requirements
- `REQ-ATM-RUNTIME-*` for daemon-client/runtime-entry requirements
- `REQ-ATM-ERROR-*` for CLI/runtime error-presentation requirements

Initial crate requirement IDs:

- `REQ-ATM-CMD-001` `atm` owns clap parsing, flag validation, and command
  dispatch for the retained command surface. Satisfies the CLI
  entry/parse/dispatch aspects of:
  `REQ-P-SEND-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-TEAMS-001`,
  `REQ-P-MEMBERS-001`.
- `REQ-ATM-OUT-001` `atm` owns human-readable and JSON rendering for retained
  commands. Satisfies the output-shaping and rendering aspects of:
  `REQ-P-SEND-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`, `REQ-P-CLEAR-001`,
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-TEAMS-001`,
  `REQ-P-MEMBERS-001`.
- `REQ-ATM-OBS-001` `atm` owns concrete observability bootstrap and injection
  into `atm-core`. Satisfies the CLI bootstrap/injection aspects of:
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`.
- `REQ-ATM-RUNTIME-001` `atm` owns CLI-to-runtime request mapping and daemon
  client use in production while preserving in-process testability. Satisfies
  the CLI/runtime-entry aspects of:
  `REQ-CORE-DAEMON-002`, `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-ATM-RUNTIME-002` `atm` owns production daemon-unavailable behavior and
  must not auto-spawn the daemon. Satisfies:
  `REQ-CORE-DAEMON-003`.
- `REQ-ATM-ERROR-001` `atm` owns CLI-side rendering/preservation of typed
  runtime errors from `atm-core` and `atm-daemon`. Satisfies:
  `REQ-CORE-BOUNDARY-002`.

`REQ-ATM-OBS-001` additionally requires:

- initializing the concrete shared logger once per CLI process
- mapping ATM env/config decisions into shared logger configuration
- consuming the published `sc-observability = "1.0.0"` crate baseline rather
  than a local pre-publish checkout
- exposing one structured construction contract for the concrete adapter:
  - `CliObservability::new(home_dir, CliObservabilityOptions)`
- keeping `init(...)` only as a delegating CLI bootstrap helper
- retaining dynamic dispatch and the current sealed-trait pattern unless
  implementation surfaces a concrete defect
- logging CLI bootstrap, parse, and terminal command failures with stable
  ATM-owned error codes before exit
- using the single ATM-owned code registry defined by
  [`../atm-error-codes.md`](../atm-error-codes.md) rather than local ad hoc
  code strings
- keeping `atm --help` / `atm send --help` aligned with the active post-send
  hook config surface; the CLI help references the ATM-owned hook semantics,
  while `atm-core` owns the underlying matching and migration behavior

## 4. Command Ownership

Per-command documentation lives under:

- [`commands/send.md`](./commands/send.md)
- [`commands/read.md`](./commands/read.md)
- [`commands/ack.md`](./commands/ack.md)
- [`commands/clear.md`](./commands/clear.md)
- [`commands/log.md`](./commands/log.md)
- [`commands/doctor.md`](./commands/doctor.md)
- [`commands/teams.md`](./commands/teams.md)
- [`commands/members.md`](./commands/members.md)

Each command document defines:

- CLI-owned flags and parsing rules
- CLI-to-core mapping
- output rendering behavior
- references to the relevant product and `atm-core` requirements

## 5. Required References

The `atm` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../plan-phase-Q.md`](../plan-phase-Q.md)

## 6. Phase Q CLI Runtime Rules

Requirement ID:
- `REQ-ATM-RUNTIME-001`

Required Phase Q rules:
- in production, `atm` acts as a client of the runtime/daemon API rather than
  talking to SQLite or inbox JSONL directly
- `atm` must not contain business logic that duplicates `atm-core`
- `atm` test coverage must be able to use in-process harnesses rather than
  depending on daemon process spawning
- if a direct in-process service harness exists for tests, it must not become a
  second production path with divergent semantics
- if the daemon is unavailable in production, `atm` must fail clearly with
  recovery guidance rather than auto-spawning or silently bypassing the daemon
- `atm doctor` remains a CLI command, but its production runtime checks may
  query daemon state through the runtime boundary
- CLI runtime failures must preserve typed error identity until the rendering
  boundary instead of collapsing into ad hoc panic/unwrap behavior
