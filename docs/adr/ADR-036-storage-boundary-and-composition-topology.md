# ADR-036 — Storage Boundary And Composition Topology

| Field | Value |
| --- | --- |
| ID | ADR-036 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-018, ADR-032, ADR-034, Phase AI, Phase AN |

## Decision

Only `atm-storage-rusqlite` may import `rusqlite`, own SQL/schema behavior, or
return a concrete SQLite backend. `atm-storage` owns backend-neutral domain
types and storage traits. `atm-core`, `atm-daemon`, `atm-graft`, HTTP adapters,
and HTTPS adapters operate only on those traits.

`atm-runtime` is a thin composition crate. It may assemble trait objects from a
selected backend, but it may not expose concrete SQLite types, add a
SQLite-specific runtime service, or define a second persistence trait for a
daemon feature. In particular, `RemoteReplayStore`,
`RuntimeStorageFinalizer`, `SqliteRemoteReplayStore`, and their composition
fields are retired.

The executable composition root selects the backend through one
backend-neutral factory/assembly input. A new backend therefore implements the
same storage traits and is selected at composition; daemon, CLI, graft, and
transport source remain unchanged.

Storage-owned domain contracts include canonical-message lookup by exact ULID
and the bounded peer/direction/age query required by ADR-038. These are
backend-neutral query methods, not SQLite tables or a daemon-owned persistence
trait. Transport adapters receive canonical records only through those traits.

The acknowledgement admission operation is one existing sealed,
backend-neutral storage/runtime contract: it resolves source data, inserts the
canonical acknowledgement, and conditionally transitions the source in one
transaction. It returns typed domain outcomes, never SQLite rows or a
daemon-feature-specific persistence trait. `atm-daemon` composition may
publish an immutable admission runtime-view snapshot assembled from
backend-neutral configuration/roster/trust DTOs, but that view is a read-only
cache and cannot own durable delivery state or concrete backend types.

### Phase AN extension — template composition and durable query capabilities

This section is the follow-up ADR required by ADR-018 §3 before Phase AN adds
its fourth and fifth optional storage capabilities. The existing optional
capabilities are `PeerConfigStore`, `OutboundMessageQuery`, and
`NudgeTemplateOverrideStore`; Phase AN may add exactly these two additional
sealed, backend-neutral capabilities:

- `TemplateCatalogStore`, for immutable content-addressed template
  registration, exact-SHA load, and non-unique type discovery;
- `MessageSearchStore`, for bounded typed message search, simple aggregate
  requests, and typed pages/results.

They are separate because catalog persistence and corpus querying are
independently reusable contracts. They must use leaf `atm-storage` DTOs and
may not accept renderer handles, HTTP DTOs, SQL strings, raw FTS syntax, or
concrete SQLite values. This extension authorizes no further optional storage
capability trait: another one requires a new ADR under ADR-018 §3.
`AsyncMessageSearchStore` is the required Tokio-safe async companion of
`MessageSearchStore`, not another semantic storage capability: it carries the
same typed query/page contract while the selected backend owns bounded reader
execution, deadline, and cancellation behavior.

`atm-template-sc-compose` is the only approved adapter crate for the pinned
upstream `sc-composer` library. It implements the core-owned
`TemplateComposer` port and may perform raw-byte hashing, frontmatter/
include-reference inspection, variable resolution, and rendering. No storage,
CLI, HTTP, daemon, runtime, or core crate may depend on it directly. Only
`atm-daemon-bootstrap` constructs it through the approved `atm-runtime`
assembly input; architecture tests must name and reject every other direct
workspace edge to the adapter.

For cross-host writes, decomposed templates and their vars never leave the
local host: the sender sends the verification render as ordinary plain text.
Likewise, a detected include/import/from-import reference never becomes a
catalog registration or decomposed row. The sender WARNs and sends the
verified plain render; if the include target cannot be resolved for that
render, the write fails closed with a typed error and performs no durable
admission. Include-graph pinning and portable reproduction remain owned by
sc-compose/dolt, not ATM.

## Consequences

The prior runtime indirection is not a justification for SQLite-backed daemon
state. Phase AI deletes it while preserving normal storage shutdown through a
storage-owned lifecycle method if one remains necessary. Architecture checks
reject `rusqlite` or `atm-storage-rusqlite` dependencies outside the concrete
backend and approved composition construction site, and reject any new
daemon-specific persistence trait.
