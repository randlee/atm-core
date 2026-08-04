# ATM Crate Architecture

> The CLI remains transport-neutral. ADR-047's trusted-LAN plain-HTTP
> production delivery and the inactive AK.6 curl mTLS fixture are daemon-side
> concerns; atm must not depend on either transport implementation.

## 1. Purpose

This document defines the `atm` crate architectural boundary.

It complements the product architecture in
[`../architecture.md`](../architecture.md) and owns only CLI-layer decisions.

The crate-local machine-readable boundary inventory lives in:
- [`./boundaries.md`](./boundaries.md)

The Phase AI target daemon API contract lives in:
- [`../atm-daemon/http-api.md`](../atm-daemon/http-api.md)

## 2. Responsibilities

The `atm` crate is responsible for:

- clap argument parsing
- command dispatch into `atm-core`
- output selection and rendering
- process exit status mapping
- constructing and injecting the concrete observability adapter
- constructing the production daemon client / runtime request adapter
- maintaining the retained CLI subcommand surface, including `teams` and
  `members`
- maintaining the approved additive `atm help` conceptual-help surface
- maintaining the queue-inspection command split where `atm list` is the
  metadata-search surface and `atm read` is the single-message detail surface

The `atm` crate must remain thin.

Phase AI target:
- the CLI depends on `DaemonApiClient` and transport-neutral application DTOs,
  never daemon internals or SQLite adapters
- send and ack create the same canonical `WriteRequest`; ack only populates
  `acknowledges_message_id`
- all daemon-backed CLI operations use ADR-033's HTTP/UDS API; the custom-frame
  helper and packet family are historical through AI.5 and must not be extended

## 1.1 ADRs

## CLI uses one daemon API client only

```yaml
adr_id: ADR-ATM-001
crate: atm
title: CLI uses one daemon API client only
status: accepted
date: 2026-05-03
deciders:
  - team-lead
  - arch-ctm
tags:
  - protocol
  - transport
  - privacy
related_boundaries:
  - BOUNDARY-DaemonApiClient
code_references:
  - docs/atm/boundaries.md
  - docs/atm-core/boundaries.md
```

Context:
- Earlier SQLite/daemon drift showed that letting CLI reach daemon internals or SQLite
  adapters made architecture violations easy and review expensive.

Decision:
- The CLI depends on `DaemonApiClient` and transport-neutral application DTOs
  only.
- It must not depend on daemon internals or SQLite adapter crates.
- Ack is the canonical write request with `acknowledges_message_id` populated;
  it has no separate client or transport path.

Consequences:
- CLI runtime wiring stays thin.
- Thin extension crates can mirror the same client shape without importing CLI
  internals.
- Thin extension crates can also version-skew from the primary `atm` install as
  long as they remain compatible with the documented same-host HTTP API.

Alternatives considered:
- Let CLI call daemon internals directly.
- Let CLI use concrete SQLite adapters for local shortcuts.

Follow-up work:
- Enforce the forbidden dependency edges in lint.
- Keep CLI help and request mapping aligned with the thin-client shape.

## 3. Architectural Rules

- `atm` may validate CLI syntax, but not reimplement `atm-core` business rules.
- `atm` may shape output, but not change core service semantics.
- `atm` owns mapping of CLI flags to `atm-core` request structs.
- `atm` owns mapping of CLI commands to the daemon/service request boundary in
  production.
- `atm` owns `--stdin` materialization before daemon bootstrap; the daemon HTTP
  surface must never receive a deferred `stdin` marker or any instruction to
  read process stdin on behalf of the caller.
- `atm` owns the explicit mailbox-surface split where `peek` and `list` are
  inspection-only, while `send`, `read`, `ack`, and `clear` are owner-only
  mutating commands.
- `atm` must not expose caller impersonation on mutating mailbox/message
  commands.
- `atm` owns bootstrap of shared observability implementations used by
  `atm-core`.
- `atm` owns the concrete published-crate bootstrap against
  `sc-observability = "1.0.0"`.
- `atm` owns retained-log bootstrap against the host-scoped ATM log directory
  contract (`~/.atm/logs/` by default, `ATM_LOG_DIR` when overridden) rather
  than `.local/share/logs` or any `ATM_HOME`-derived path.
- `atm help` is CLI-owned conceptual help layered over clap command help and
  must delegate command flag truth to clap output instead of maintaining a
  parallel flag-documentation source.
- `atm help` must surface installed-doc pointers for long-form operator
  guidance; the CLI owns the pointer/rendering seam, but not the long-form
  document corpus itself.
- installed-doc lookup for `atm help` is executable-relative from the resolved
  installed `atm` binary location and must not be derived from `ATM_HOME`.
- the installed-user-documentation-surface follows ADR-025:
  - the repo-owned source corpus is `docs/user-documents/`
  - packaging installs that corpus under `<install-root>/share/doc/atm/`
  - long-form help lookup resolves from the installed binary using the
    executable-relative path `../share/doc/atm/`
- `atm` owns the structured construction contract for the concrete adapter:
  `CliObservability::new(home_dir, CliObservabilityOptions)`.
- `atm` may retain `init(...)` only as a delegating helper.
- `atm` owns CLI-layer observability for command entry, daemon connectivity,
  and render/exit outcomes.
- `atm` owns the shipped built-in `internal-nudge` command, the hidden-command
  compatibility parsing around its resolved envelope, and the final built-in
  sink dispatch into `TmuxNudgeSink` or `GraftNudgeSink`.
- the accepted built-in template catalog and placeholder renderer are shared
  helper semantics supplied through `atm-core`; `atm` must not fork that
  catalog or reintroduce a second selection path
- retained `atm internal-nudge` helper invocations consume one resolved
  envelope from `ATM_INTERNAL_NUDGE`; they render and deliver only and must
  not reopen `NudgeTemplateOverrideStore`, SQLite, or runtime bootstrap
  composition.
- the self-addressed-send rejection rule remains `atm-core` business logic;
  `atm` must route all send entry paths, including `--dry-run`, through that
  shared validation contract and render the returned typed failure rather than
  introducing a CLI-local mailbox-equality rule
- `atm` may consume a team-scoped built-in template override body only through
  the storage-neutral upstream contract accepted for Phase `AD`; it must not
  perform direct SQLite lookup itself.
- `atm` resolves built-in template lifecycle through that upstream contract as:
  no row => product default, override row => stored body, disabled row => no
  emission, clear/reset => row deletion back to product default.
- `atm` owns `TmuxNudgeSink`, including the current tmux-injection sequence:
  paste rendered text, send `Enter`, wait about `250ms` to `300ms`, then send
  a second `Enter`; the exact delay remains implementation-tunable but the
  accepted design must preserve and verify the double-enter behavior.
- `atm` owns the default retained logger baseline needed to keep daemon
  lifecycle `info!` events and all `warn!` / `error!` events visible when
  `ATM_LOG` is unset.
- `atm` owns the retained local recovery CLI shape for `teams` and `members`,
  but not the underlying team/backup/restore business rules
- `atm` must not access SQLite or inbox JSONL directly
- `atm` must not own socket protocol semantics beyond client-side request
  mapping and error presentation
- `atm` must own the one documented daemon auto-start path in production and
  must not silently bypass the daemon if startup fails
- daemon auto-start must be an explicit runtime-entry step, not a hidden side
  effect of transport object construction
- the client-side launch path must acquire the documented pre-spawn launch gate
  before daemon fork/exec
- the CLI standard same-host bootstrap path must be the canonical thin-client
  convenience wrapper for endpoint resolution, daemon-binary resolution, probe,
  and supervised auto-start through the shared `atm-daemon-client` bootstrap
  seam; other first-party thin clients may mirror that path, but they must not
  depend on CLI internals to do so
- the canonical thin-client bootstrap seam must stay free of runtime/storage
  composition ownership so convenience auto-start never forces `atm-runtime` or
  concrete backend crates into thin-client dependency graphs
- `atm` must preserve typed runtime error identity until the rendering
  boundary instead of collapsing failures into panic/unwrap control flow
- `atm` must keep built-in acknowledge nudges compact:
  - `<atm kind="ack" from="..." message-id="..."/>`
  - `<atm kind="ack" from="..." message-id="..." task-id="..."/>`

## 3.1 Phase R CLI / Runtime Split

Phase R keeps the CLI thin by enforcing this split:

- `atm` owns parse -> request mapping -> render
- `atm-core` owns business logic and service semantics
- `atm-daemon` owns runtime transport and singleton behavior

Test strategy rule:
- CLI tests must be able to target an in-process harness without requiring a
  daemon process
- `CliComposition::from_transport(...)` is the primary seam for fake or
  loopback transport tests

Doctor/runtime rule:
- `atm doctor` remains a CLI command, but its runtime-facing checks must query
  daemon state through the same daemon/service boundaries used by production

## 4. ADR Namespace

The `atm` crate uses the `ADR-ATM-*` namespace.

Initial use cases:

- clap surface decisions
- output-format decisions
- observability bootstrap wiring
- command-dispatch structure
