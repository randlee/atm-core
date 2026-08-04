# ATM Crate Requirements

> **AK.6 status:** atm stays transport-neutral while ADR-047 supersedes the
> legacy TLS/authority/outcome mechanics. The inactive atm-peer-tls-interop
> fixture is not a CLI dependency.

## 1. Purpose

This document defines the `atm` crate requirements.

The `atm` crate owns the CLI layer and the CLI-side daemon client only.
Product behavior remains defined in [`../requirements.md`](../requirements.md).
`atm` must satisfy those product requirements without re-owning `atm-core`
business logic or `atm-daemon` runtime behavior.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

The canonical daemon HTTP contract lives in:
- [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)

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
- the requirement that all first-party thin clients ship at the exact same
  crate version

## 3. Requirement Namespace

The `atm` crate uses the `REQ-ATM-*` namespace.

Initial allocation:

- `REQ-ATM-CMD-*` for command-entry requirements
- `REQ-ATM-OUT-*` for output/rendering requirements
- `REQ-ATM-OBS-*` for observability-bootstrap requirements
- `REQ-ATM-RUNTIME-*` for daemon-client/runtime-entry requirements
- `REQ-ATM-TRANSPORT-*` for CLI-to-daemon peer-delivery requirements
- `REQ-ATM-ERROR-*` for CLI/runtime error-presentation requirements

Initial crate requirement IDs:

- `REQ-ATM-CMD-001` `atm` owns clap parsing, flag validation, and command
  dispatch for the retained command surface. Satisfies the CLI
  entry/parse/dispatch aspects of:
  `REQ-P-SEND-001`, `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`,
  `REQ-P-CLEAR-001`, `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-TEAMS-001`,
  `REQ-P-MEMBERS-001`, `REQ-P-HELP-001`.
- `REQ-ATM-OUT-001` `atm` owns human-readable and JSON rendering for retained
  commands. Satisfies the output-shaping and rendering aspects of:
  `REQ-P-SEND-001`, `REQ-P-LIST-001`, `REQ-P-READ-001`, `REQ-P-ACK-001`,
  `REQ-P-CLEAR-001`, `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-TEAMS-001`,
  `REQ-P-MEMBERS-001`, `REQ-P-HELP-001`, `REQ-P-USER-DOCS-001`.
- `REQ-ATM-OBS-001` `atm` owns concrete observability bootstrap and injection
  into `atm-core`. Satisfies the CLI bootstrap/injection aspects of:
  `REQ-P-LOG-001`, `REQ-P-DOCTOR-001`, `REQ-P-OBS-001`, `REQ-P-OBS-002`,
  `REQ-P-OBS-003`.
- `REQ-ATM-RUNTIME-001` `atm` owns CLI-to-runtime request mapping and daemon
  client use in production over the shared HTTP `DaemonApiClient` contract while
  preserving in-process testability. Satisfies the CLI/runtime-entry aspects of:
  `REQ-CORE-DAEMON-002`, `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-ATM-RUNTIME-002` `atm` owns production daemon-unavailable behavior and
  the one documented daemon auto-start attempt. Satisfies:
  `REQ-P-RUNTIME-001`, `REQ-CORE-DAEMON-001`.
- `REQ-ATM-RUNTIME-003` `atm` owns the client-side pre-spawn launch gate for
  daemon auto-start and must serialize concurrent CLI spawn attempts before
  fork/exec. Satisfies:
  `REQ-P-RUNTIME-002`, `REQ-P-RUNTIME-003`.
- `REQ-ATM-RUNTIME-004` `atm` owns CLI test seams for runtime access and must
  support injected in-process transport doubles without real daemon spawning as
  the ordinary test path. Satisfies:
  `REQ-P-TEST-001`, `REQ-CORE-TEST-RUNTIME-001`.
- `REQ-ATM-TRANSPORT-002` `atm` submits every host-qualified `send` to its
  local daemon through the same daemon HTTP request contract as a local send.
  It does not resolve DNS, open a peer socket, run a retry, or create delivery
  state. After durable local admission, the daemon owns exactly one bounded
  direct configured-peer HTTP attempt; success is a matching peer response and
  failure remains persisted-but-undelivered. Satisfies:
  `REQ-CORE-TRANSPORT-002`, `REQ-CORE-TRANSPORT-003`,
  `REQ-CORE-TRANSPORT-004`.
- `REQ-ATM-TRANSPORT-003` `atm peer resend-cache {show,set <true|false>}`
  reads or persists the one daemon setting only. It must not open peer sockets,
  inspect `peerOutbound`, run a timer, or retry a message. Satisfies:
  `REQ-CORE-TRANSPORT-003`, `ADR-046`.
- `REQ-ATM-ERROR-001` `atm` owns CLI-side rendering/preservation of typed
  runtime errors from `atm-core` and `atm-daemon`. Satisfies:
  `REQ-CORE-BOUNDARY-002`.

`REQ-ATM-OBS-001` additionally requires:

- initializing the concrete shared logger once per CLI process
- mapping ATM env/config decisions into shared logger configuration
- resolving the retained log directory through the host-scoped ATM log-path
  contract rather than through `ATM_HOME`
- honoring `ATM_LOG_DIR` as the exact retained log-directory override
- consuming the published `sc-observability = "1.0.0"` crate baseline rather
  than a local pre-publish checkout
- exposing one structured construction contract for the concrete adapter:
  - `CliObservability::new(home_dir, CliObservabilityOptions)`
- keeping `init(...)` only as a delegating CLI bootstrap helper
- retaining dynamic dispatch and the current sealed-trait pattern unless
  implementation surfaces a concrete defect
- logging CLI bootstrap, parse, and terminal command failures with stable
  ATM-owned error codes before exit
- keeping the default retained logger baseline high enough to preserve daemon
  lifecycle `info!` events plus all `warn!` / `error!` events when `ATM_LOG`
  is unset
- using the single ATM-owned code registry defined by
  [`../atm-error-codes.md`](../atm-error-codes.md) rather than local ad hoc
  code strings
- keeping `atm --help` / `atm send --help` aligned with the active post-send
  hook and built-in nudge surface; the CLI help references the shipped
  built-in behavior plus the retained `atm internal-nudge` helper and the
  external override semantics, while `atm-core` owns the underlying matching
  and migration behavior
- keeping `atm help` topic rendering aligned with the installed end-user
  corpus so the CLI points to `<install-root>/share/doc/atm/` for long-form
  operator docs
- resolving installed-doc pointers for `atm help` from the installed binary
  location using the executable-relative path `../share/doc/atm/README.md`
  rather than from `ATM_HOME`

## 3.1 Built-In Nudge Surface

Requirement ID:
- `REQ-ATM-NUDGE-001`

Required rules:
- `atm` owns the retained hidden `atm internal-nudge` helper surface
- the shipped built-in post-send implementation stays on the in-process daemon
  / emitter line
- `atm internal-nudge`, when invoked, must consume one resolved built-in
  template envelope
  carrying exactly one built-in template kind:
  - `delivery`
  - `delivery_ack`
  - `delivery_task`
  - `delivery_task_ack`
  - `acknowledge`
  - `acknowledge_task`
- `atm` owns direct placeholder substitution for those templates; no Jinja or
  conditional template language is allowed on the built-in path
- `atm internal-nudge` must read the resolved built-in envelope from
  `ATM_INTERNAL_NUDGE`; that envelope carries:
  - the canonical `PostSendHookEvent`
  - the concrete sink target
  - the resolved template kind
  - the resolved template body or explicit disabled state
- the accepted built-in path is bounded to six default template bodies, but any
  team-scoped override lookup for those bodies must cross the storage-neutral
  `NudgeTemplateOverrideStore` contract upstream of `PostSendHookEmitter`
  rather than performing direct SQLite access or runtime/store reopening in
  the CLI crate
- `atm` must preserve the shared self-addressed-send rejection contract across
  every CLI send entry path, including `--dry-run`; when the canonical sender
  and resolved recipient are the same same-team member, the CLI surfaces the
  typed validation failure returned by `atm-core` instead of reporting send
  success
- built-in precedence is:
  - matching external `[[atm.post_send_hooks]]` command
  - resolved team-scoped template row returned through the upstream
  `NudgeTemplateOverrideStore` contract for the selected template kind
  - built-in product default template body for that kind when no row exists
- resolved row semantics are:
  - no row => built-in product default
  - override row => use the stored non-empty template body
  - disabled row => emit no built-in nudge
  - clear/reset => delete the row so the next lookup returns product default
- empty-string template bodies are invalid; operators must use explicit
  disable or clear commands instead
- the default built-in acknowledge template bodies are:
  - `<atm kind="ack" from="{{from}}" message-id="{{message_id}}"/>`
  - `<atm kind="ack" from="{{from}}" message-id="{{message_id}}" task-id="{{task_id}}"/>`
- the built-in path must not access SQLite directly; the first concrete
  host-scoped override storage remains `atm-storage-rusqlite` implementation
  detail behind the accepted `atm-core` contract

## 3.2 Native Send Input Materialization

Requirement ID:
- `REQ-ATM-CMD-003`

Required rules:
- `atm` owns `--stdin` as a CLI-only input source and must consume it before
  daemon bootstrap and before request dispatch over the same-host HTTP API
- a daemon-bound send request may encode only durable inline bytes or the
  retained `--file` reference contract; it must never encode a `stdin`
  instruction for the daemon to resolve later
- invalid `--stdin` input (empty, whitespace-only, oversized, unreadable, or
  non-UTF-8) must fail at the CLI boundary with the typed ATM error returned by
  `atm-core`
- invalid `--stdin` input must not start a daemon and must not dispatch a
  `DaemonApiClient` request

## 4. Command Ownership

Per-command documentation lives under:

- [`commands/send.md`](./commands/send.md)
- [`commands/list.md`](./commands/list.md)
- [`commands/read.md`](./commands/read.md)
- [`commands/ack.md`](./commands/ack.md)
- [`commands/clear.md`](./commands/clear.md)
- [`commands/log.md`](./commands/log.md)
- [`commands/doctor.md`](./commands/doctor.md)
- [`commands/teams.md`](./commands/teams.md)
- [`commands/members.md`](./commands/members.md)
- [`commands/help.md`](./commands/help.md)

Each command document defines:

- CLI-owned flags and parsing rules
- CLI-to-core mapping
- output rendering behavior
- references to the relevant product and `atm-core` requirements

## 4.1 Mailbox Inspection And Mutation Split

Requirement ID:
- `REQ-ATM-CMD-002`

Required rules:
- `atm peek` is the explicit non-mutating mailbox inspection command
- `atm list` remains a non-mutating mailbox metadata query
- only inspection-only surfaces may accept `--as`
- `atm send`, `atm read`, `atm ack`, and `atm clear` are owner-only mutating
  commands and must not expose mutating impersonation flags
- CLI request mapping must preserve that split when it constructs
  `atm-core` request DTOs

## 5. Required References

The `atm` crate docs must remain aligned with:

- [`../requirements.md`](../requirements.md)
- [`../architecture.md`](../architecture.md)
- [`../atm-error-codes.md`](../atm-error-codes.md)
- [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)
- [`../project-plan.md`](../project-plan.md)
- [`../documentation-guidelines.md`](../documentation-guidelines.md)
- [`../plan-phase-R.md`](../plan-phase-R.md)
- [`../plan-phase-S.md`](../plan-phase-S.md)
- [`../testing-guidelines.md`](../testing-guidelines.md)
- [`./boundaries.md`](./boundaries.md)

## 6. Phase R CLI Runtime Rules

Requirement ID:
- `REQ-ATM-RUNTIME-001`

Required Phase R rules:
- in production, `atm` acts as a client of the runtime/daemon API rather than
  talking to SQLite or inbox JSONL directly
- in production, `atm` depends on the shared HTTP `DaemonApiClient` contract
  rather than daemon internals
- the daemon HTTP resource surface currently covers:
  - `send`
  - `ack` through the send-shaped acknowledge request
  - `list`
  - `read`
  - `clear`
  - `doctor`
- the retained CLI surfaces `log`, `teams`, and `members` are not daemon
  request/response packets in the current Phase S line
- `atm` must not contain business logic that duplicates `atm-core`
- `atm` test coverage must be able to use in-process harnesses rather than
  depending on daemon process spawning
- `atm help` remains an additive CLI-owned conceptual-help surface layered on
  top of the retained command set; it must not become a general structured
  JSON-input expansion point inside `Phase Y`
- `atm help` topic output must stay concise and point to installed end-user
  docs rather than trying to inline the full operator corpus
- `atm` owns legacy queue-flag deprecation warnings and the exact
  `atm read --message-id <id>` retrieval guidance shown for ATM-authored JSONL
  body stubs
- `atm` owns the queue-inspection CLI split where:
  - `atm list` parses shared filters and renders bounded metadata rows
  - `atm read` parses the same shared filters, resolves one selected message,
    and renders match metadata when additional matches remain
- daemon auto-start is a supervised runtime entry concern, not a side effect of
  transport object construction
- the CLI-side auto-start path must acquire the documented pre-spawn launch
  gate before daemon fork/exec so concurrent CLIs cannot race into second-daemon
  startup attempts
- the CLI standard same-host bootstrap path must reuse the shared
  `atm-daemon-client` endpoint/bin helper seam rather than owning a
  CLI-private bootstrap helper surface
- CLI tests must not rely on `warm_daemon`, `ATM_DAEMON_BIN`, or other daemon
  spawn helpers to exercise normal command behavior
- `CliComposition` retains a primary seam for injected fake HTTP-client and
  in-process HTTP-adapter tests
- if a direct in-process service harness exists for tests, it must not become a
  second production path with divergent semantics
- if the daemon is unavailable in production, `atm` must:
  - attempt the one documented daemon auto-start path
  - retry connection once
  - fail clearly with recovery guidance rather than silently bypassing the daemon
- the documented daemon auto-start path is also the canonical first-party
  thin-client convenience path:
  - it may be reused by crates such as `atm-graft`
  - it resolves ATM-owned environment/config inputs into the canonical
    same-host endpoint and daemon binary
  - it is allowed to start the daemon when launch conditions are met
  - it must not require a compile-time dependency on `atm-runtime`,
    `atm-storage-rusqlite`, or other concrete backend composition crates
- compatibility between first-party thin clients and the primary `atm` install
  is governed by the documented same-host HTTP API rather than lockstep
  crate-version equality
- `atm doctor` remains a CLI command, but its production runtime checks may
  query daemon state through the runtime boundary
- CLI runtime failures must preserve typed error identity until the rendering
  boundary instead of collapsing into ad hoc panic/unwrap behavior
- CLI-owned caller-context parsing and precedence must follow the authoritative
  matrix in `docs/requirements.md` §4.1 exactly, including the `atm doctor`
  exception and the rule that explicit CLI override wins over env when both are
  present
