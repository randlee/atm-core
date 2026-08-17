---
title: Phase AL Plan — `atm-http-runtime`
status: complete
branch: develop
worktree: /Users/randlee/Documents/github/atm-core
baseline: develop @ 67401907039f92e58e883273f02372a637202f70
---

# Phase AL — Build the Minimal Tokio HTTP Runtime

## Goal

Replace ATM's hand-written synchronous HTTP framing and transport-specific
request processing with one small `atm-http-runtime` library. It uses Tokio and
maintained HTTP/TLS libraries to provide the same typed application contract to
all clients and all listeners.

AL is an additive replacement phase. It does not preserve the legacy transport
as a compatibility architecture and does not add resend/replay. Phase AM
deletes the legacy implementation once AL proves the replacement.

## Baseline and entry gate

- Implementation starts from the current `develop` baseline recorded above,
  which includes the completed Phase AJ merge.
- **Archived AK.11 is a source, not a prerequisite.** AL.1 starts from
  `develop` and directly copies the approved receiver-hook design from archived
  AK.11 commit `88bca9d5e232006339f43a4e97eef335531b8a8f` (plus only any
  hook-local follow-up required by its tests). It must preserve the exact
  sealed trait, receiver implementations, post-persistence invocation, and
  warning/duplicate semantics. Do not revive or complete Phase AK.
  `88bca9d5` is a mixed 51-file AK commit, so AL.1 may **not** cherry-pick it
  wholesale: it copies/cherry-picks only the hook-boundary file set and tests,
  records the source SHA, and rejects every unrelated peer transport, replay,
  listener, or scheduler hunk. The historical `PostSendHookEmitter` is not an
  active interface after that narrow transplant.
- **AK.11 state for this plan:** `archived_reference_source` at
  `88bca9d5`; it does not block AL.1 and does not authorize any other AK code.
- The exact guardrails are in
  [`phase-al-am-runtime-boundary-checklist.md`](../phase-al-am-runtime-boundary-checklist.md).
  Every AL PR must pass them before merging forward.

### Integration-line policy

`plan/tokio-migration` is the planning/integration line until AL.1 creates the
implementation integration line. The named AL integrator synchronizes that
line with `develop` before each sprint starts and at least weekly while a
sprint remains open, resolves conflicts on the integration line, and reruns
the sprint's required checks after every sync. Children merge forward from a
pushed parent commit as their `must_follow` metadata specifies; they do not
rebase a shared pushed line. AL.9 freezes the exact accepted integration SHA
for the physical proof and AM ledger; later `develop` drift requires a new
approved proof round before AM deletion.

## Architecture

`atm-http-runtime` is the replacement Tokio implementation and ships the
active `atm-daemon` executable target. It is not a second concurrently running
daemon: `crates/atm-daemon` is reference-only legacy source and is never
started, wrapped, used for smoke/proof, or retained as fallback. Phase AM
deletes that legacy implementation after the replacement's accepted physical
proof.

```text
atm / atm-graft / cross-host sender
    └── shared typed HTTP client
            └── existing route-specific JSON types and serialization
                    └── Tokio HTTP listener(s)
                            └── one typed /v1/atm/messages handler
                                    └── ApiRouter + injected core boundaries
                                            └── storage trait
                                            └── MessageReceivedHookEmitter

atm-http-runtime = active daemon process, listener selection, lifecycle
legacy atm-daemon = reference-only source pending AM deletion; never executed
```

### Library choices

- **Tokio** owns asynchronous execution, listener lifecycle, cancellation, and
  bounded task execution.
- **Axum/Hyper** provide routing, JSON extraction, response construction, and
  HTTP protocol handling. ATM must not parse or frame HTTP itself.
- **Rustls** provides the authenticated TLS configuration for physical peer
  links. TLS identity contributes authenticated ingress provenance; it does not
  select a separate application route.
- A maintained Tokio HTTP client is used by both local adapters and
  cross-host sends. Connector setup may differ for UDS, loopback TCP, and
  TLS TCP; body serialization, endpoint path, request dispatch, response
  decoding, and outcome handling do not.

These choices use the maintained Tokio-compatible server/router and protocol
libraries rather than reinventing HTTP. Axum is explicitly designed to run on
Tokio/Hyper and provides typed routing and request extraction; Hyper provides
the async HTTP implementation. [Axum documentation](https://docs.rs/axum/latest/axum/)
and [Hyper documentation](https://docs.rs/hyper/latest/hyper/) are the
implementation references; versions are selected and lockfile-pinned during
AL.1.

### Fixed boundaries

- **Transport-struct and serialization freeze.** AL preserves the current
  public route-specific request, success-result, warning, and ADR-032 error
  types *and their existing Serde/OpenAPI serialization*. It adds no wrapper,
  peer DTO, envelope variant, array grammar, field, header-as-body contract,
  schema migration, or compatibility codec. `RequestEnvelope` and
  `ResponseEnvelope`, where used today, remain internal application values;
  ADR-033 explicitly forbids exposing them as a generic HTTP wire envelope.
  `WriteRequest` is never wrapped in a peer-only type or array grammar.
- `POST /v1/atm/messages` invokes one typed handler. Local and peer calls
  differ only in connector setup and trusted ingress provenance. The handler
  dispatches through the existing core `ApiRouter`, not a runtime-private
  decoder or dispatcher.
- The daemon and runtime use the existing sealed core storage boundaries; they
  never reference SQLite or a `rusqlite` implementation.
- After a newly persisted inbound message only, the shared dispatch path calls
  the injected `MessageReceivedHookEmitter`. A hook failure produces retained
  warning data but cannot turn a successful persistence/receive into failure.
- Tmux and graft remain receiver implementations selected outside the runtime.
  The daemon does not import `atm-graft`; the runtime imports neither harness.
- No resend/replay is implemented in AL. A future opt-in replay feature, if
  authorized, starts only after minimal direct cross-host proof and uses the
  same endpoint and types.

The implementation details, exact requirement/ADR mapping, and proof ownership
are binding in
[`phase-al-am-requirement-adr-traceability.md`](../phase-al-am-requirement-adr-traceability.md)
and
[`phase-al-am-runtime-design.md`](../phase-al-am-runtime-design.md). A sprint
may not reinterpret an old delivery-state requirement into an AL feature; the
traceability record names its disposition instead.

Boundary changes are governed by
[`phase-al-am-boundary-transition.md`](../phase-al-am-boundary-transition.md):
they land with the corresponding implementation, never as advance planning
edits or a broad boundary cleanup.

## Sprints

### AL.1 — Accepted hook-contract transplant and runtime crate skeleton

**Depends on:** `develop` plus the narrow exact-copy/cherry-pick of archived
AK.11 hook source `88bca9d5`; no AK branch completion or merge is required.

- Add workspace library crate `atm-http-runtime` with a minimal public API:
  typed server construction, typed client construction, listener/connector
  configuration, and graceful shutdown handle.
- Add Tokio, Axum/Hyper, Rustls, and the selected maintained client through
  workspace dependencies with minimal feature sets.
- Define the runtime lifecycle as a compile-time typestate transition:
  `HttpRuntime<Configured>` may be constructed only after pure configuration
  validation, `start` consumes it into `HttpRuntime<Running>`, shutdown enters
  `HttpRuntime<Draining>`, and only completion/cancellation yields
  `HttpRuntime<Stopped>`. Endpoint publication is impossible before `Running`;
  `start`/shutdown are not callable twice and the handles are not clonable.
- Validate all runtime configuration before binding or publishing anything:
  bind address/interface, UDS path/permissions, body/connection limits,
  deadline values, and required TLS identity/trust material. Invalid or
  contradictory input is a typed startup error with field/cause context—never
  a silent fallback or partially live listener.
- Make the runtime depend only on core interfaces and protocol types.
- Add compile-time/boundary tests proving it cannot import SQLite, tmux, graft,
  daemon-bootstrap, or resend modules.
- Create/rename/delete the exact boundary manifests, human boundary documents,
  exports, and allowed implementation entries required by the accepted hook
  transplant and new runtime facade in the same AL.1 PR. See the boundary
  transition inventory; do not pre-create them on the plan branch.
- Record the exact existing public route body/result/error type and current
  serializer entry point for every migrated route. This is a compatibility
  inventory, not a new abstraction or a schema migration.
- Before any handler work, capture the baseline's malformed-JSON,
  oversize-body, and bad-header status/body responses; confirm the existing
  warning representation and the canonical write disposition surface
  (new/idempotent duplicate/conflict). If either is unavailable without a core
  trait or public-schema change, stop for an explicit contract decision.

**Accept when:** the archived hook source SHA and exact contract are recorded;
the crate compiles; active hook type is `MessageReceivedHookEmitter`; the
warning/disposition and malformed-request compatibility oracles are recorded;
the boundary checklist is encoded in tests; no production request flows
through the new crate yet.

### AL.2 — Canonical typed HTTP handler

**Depends on:** AL.1. The AL.1 integration commit must be pushed and merged
forward before each development or fix round; AL.1 PR merge is not required.

- Implement `POST /v1/atm/messages` with framework JSON extraction of the
  exact existing route-specific body and result types, using their current
  Serde/OpenAPI serialization. Do not introduce `RequestEnvelope` or
  `ResponseEnvelope` as a generic wire format.
- Inject `ApiRouter`, storage-facing core contracts, authenticated ingress
  provenance, observability, and the received-hook boundary as explicit state.
- Ensure all authentication/provenance normalization happens before the one
  core dispatch call; there is no peer decoder or peer router.
- Add the one adapter mapping from typed core errors to the existing ADR-032
  `{code,message}` HTTP result contract without ad-hoc framing or a second
  response schema.
- Install framework rejection mapping for each case captured by AL.1, or stop
  for a reviewed contract decision. Framework defaults may not silently
  replace ADR-032 JSON with a plain-text body.

**Accept when:** local and peer fixtures dispatch identical serialized writes;
the route maps every AL.1 malformed-request oracle through the existing
ADR-032 contract; no handwritten HTTP parser/writer is added.

### AL.3 — Post-persistence received-hook semantics

**Depends on:** AL.2 and the AK.11 hook contract.

- Connect the runtime to the one post-persistence path rather than a sender
  hook or listener callback.
- Preserve idempotency: a duplicate message ID is an informational successful
  result and does not emit a second hook.
- Return successful write/receive result plus warning information when the
  injected hook fails; retain diagnostic cause locally.
- Provide a test emitter that proves invocation count and error behavior.

**Accept when:** all three hook proofs in the shared checklist pass against
the runtime handler for single and multiple independently delivered requests.

### AL.4 — Shared standard client

**Depends on:** AL.2.

- Implement one typed send function that serializes the existing route body,
  sends it to `/v1/atm/messages`, and decodes the existing route result.
- Define the connector-neutral client operation. It owns shared body
  serialization, endpoint path, response decoding, and outcome mapping only.
- Migrate the existing sealed `DaemonApiClient` operation to `#[async_trait]`
  and update all allowlisted implementations together so its `Arc<dyn ...>`
  use remains object-safe. The received-message hook remains synchronous; no
  manual future vtable, `block_on` bridge, or second client trait is allowed.
- Migrate `crates/atm-graft/src/transport.rs` outbound traffic off legacy
  `atm_daemon_client::exchange_request` / `try_connect` and onto this concrete
  shared runtime client. The daemon and runtime still do not depend on
  `atm-graft`; this is a client-consumer dependency only.
- Do not migrate a physical listener or connector in this sprint; those are
  independently reviewed in AL.5, AL.6, and AL.7.

**Accept when:** the canonical client compiles with test connectors and has
one encode/decode path; graft's outbound transport has no legacy
`exchange_request` / `try_connect` reference; no automatic retry/replay starts.

### AL.5 — Unix UDS local adapter

**Depends on:** AL.2 and AL.4.

- Move Unix local UDS client/listener setup to the framework-managed runtime.
- Prove it reaches the canonical client and handler without raw framing.

**Accept when:** Unix local CLI smoke reaches the new route; all retained
same-host request types preserve their current typed results.

### AL.6 — Loopback TCP local adapter

**Depends on:** AL.2 and AL.4.

- Move loopback TCP client/listener setup to the framework-managed runtime.
- Prove Windows-compatible loopback behavior without introducing an
  OS-specific application route or codec.

**Accept when:** loopback smoke and integration tests reach the canonical
client/handler and preserve the UDS-equivalent result contract.

### AL.7 — Authenticated peer TLS adapter and M5 lane

**Depends on:** AL.2, AL.4, and the accepted existing TLS policy.

- Add the physical authenticated TLS connector/listener configuration to the
  canonical runtime without a peer route, decoder, request body, or retry.
- Give the M5 team an isolated clean-checkout proof task that exercises one
  direct cross-host canonical write using the unchanged route body.

**Accept when:** the M5 lane proves the same client serialization and handler
dispatch as local traffic. It is not a resend/replay proof.

### AL.8 — Daemon composition and static boundary proof

**Depends on:** AL.3, AL.5, and AL.6. Each parent integration commit must be
pushed and merged forward before a development/fix round; parent PR merge is
not required. AL.7's peer TLS adapter is deferred because it is not MVP scope;
the isolated TLS crate is retained for a future authorized phase.

- Reduce `atm-daemon` integration to building trait implementations,
  selecting listener/connector configuration, starting the runtime, and
  graceful shutdown.
- Activate only Unix UDS (where supported) and loopback TCP in this MVP
  composition. AL.8 neither configures nor activates peer TLS.
- Prove in-process composition and static source/dependency boundaries:
  the daemon constructs only allowed trait implementations, selects
  adapters, starts the runtime after the existing owner gate, and uses no
  legacy framing/peer/replay or concrete storage dependency.
- Record the actual live-reference graph as input for AM.1; AM.1, not AL.8,
  owns the removal ledger and its deletion topology.

**Accept when:** composition/lifecycle and static boundary proofs pass and the
live-reference graph is captured. Physical adapter proof, benchmark gating,
M5 artifact reuse, and AM ledger freeze belong exclusively to AL.9.

### AL.9 — Physical proof, benchmark gate, and AM ledger freeze

**Depends on:** AL.8 and AL.4's accepted graft outbound-client migration.
Both pushed integration commits must be merged forward before each
development/fix round; their PR merges are not required. AM deletion cannot
start until AL.9's proof and frozen-ledger inputs are accepted.

- Run the complete MVP physical-adapter proof: in-process, Unix UDS,
  loopback, graft client, and clean-checkout M5 direct send. TLS is out of
  scope under PR #774 (`0c3bc49a`); AL.7 was never implemented, so no TLS or
  AL.7-artifact reuse may be claimed.
- Fold AL.7's skipped non-TLS local-client closure item into AL.9 for send
  only: migrate CLI and graft writes to the existing shared UDS/loopback
  `DaemonApiClient` path. Retain the synchronous compatibility client only for
  read/ack/admin dispatch and record its async conversion/deletion as explicit
  AM.1 ledger work. This changes no frozen legacy-daemon source and creates no
  retry, replay, batching, or alternate route.
- Compare against the pre-AL baseline captured at `67401907` before AL.1:
  fixed workload, hardware/OS/toolchain, p50/p99 latency and throughput,
  raw artifacts, defined tolerances, and a real Windows CI/measurement lane.
  Measure hook-active latency as well as hook-disabled latency.
- Define the cutover table for each adapter (add, activate, retire, owner,
  rollback): exactly one active listener and endpoint-record publisher per
  endpoint. If proof, benchmark, or scheduled M5 reproduction fails, legacy
  remains live, the AL line parks, AM does not start, and the ledger is not
  frozen.
- Freeze AM.1's draft ledger only against AL.8's actual live-reference graph,
  including observability/doctor/config consumers and their disposition.

**Accept when:** all physical proof artifacts and benchmark gates pass, M5
evidence is valid or explicitly reused under the rule above, the cutover table
has one active publisher per endpoint, and AM.1's ledger input is frozen.

### AL.13–AL.15 — Direct M5/cwin hardware-smoke continuation

AL.13, AL.14, and AL.15 are post-AL.9 hardware-evidence sprints. They do not
reopen AL.7's TLS scope or create a new transport feature. They use the
canonical `/smoke-test` skill and `just smoke` ladder to prove the existing
direct peer send path on real M5 and cwin machines:

- **AL.13:** M5 owns its local ladder and the M5-initiated direct peer
  send/read/ack evidence.
- **AL.14:** cwin owns its local Windows ladder and the cwin-initiated
  symmetric direct peer send/read/ack evidence.
- **AL.15:** consumes only the retained evidence and reports an unambiguous
  PASS or BLOCKED/FAIL outcome.

Every hardware operator works from its named home sprint branch, retains each
run in the platform/host/run report layout, and opens a PR with both the
status report and the evidence. The authoritative tested-candidate manifest,
allowed `just smoke` feature set, exclusion scope gate, and closeout review
are defined only by the AL.13, AL.14, and AL.15 sprint documents. Those docs
require the report-index merge from PR #788 and reject a mismatched candidate
or prohibited transport change.

## Explicitly deferred

- Automatic resend, heartbeat-driven recovery, cursor tracking, batching, and
  `message[]` delivery are not AL features.
- New notification modes and changes to tmux/graft UX are not AL features.
- Storage schema changes and daemon knowledge of SQLite are prohibited.
- Changing any public transport struct, its JSON serialization, the OpenAPI
  schema, or message/ack semantics is prohibited. Such a change requires a
  separately approved API-compatibility phase.

## Phase completion gate

AL completes only when AL.9 proves the new runtime is the active path and satisfies
every required row of the shared boundary checklist. It does not complete by
merely compiling alongside the legacy server. AM may remove the old stack only
after this gate passes.
