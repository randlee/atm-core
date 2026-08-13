# ADR-046 — Template-Declared Workflow Metadata And Admission Snapshots

| Field | Value |
| --- | --- |
| Status | Accepted for Phase AN extension planning |
| Scope | Template catalog, durable message admission, local analytics, telemetry projection |
| Relates to | ADR-001, ADR-036, ADR-045; `REQ-P-TEMPLATE-WORKFLOW-001`, `REQ-P-TEMPLATE-TAGS-001`, `REQ-P-WORKFLOW-ANALYTICS-001` |

## Context

ATM must expose durable facts from arbitrary formal workflows without encoding a
particular orchestration, team, or process. A common use case is to measure a
development/review loop: its duration, the number of review rounds, and the
time spent in remediation. The names `dev-start`, `qa-complete`, and
`fix-start` are useful conventions, but are not ATM business vocabulary.

Template revisions are immutable and content-addressed. Their metadata can be
joined cheaply today, but a catalog join answers what a template says *now*;
audits and lifecycle analytics require the classification applied when the
message was admitted. Template tags also need a distinct provenance from
sender-provided instance tags.

## Decision

### 1. Generic template declaration

A template may declare literal metadata under frontmatter `metadata`:

```yaml
metadata:
  type: dev-task
  tags: ["domain:orchestration", "audience:engineering"]
  workflow:
    scope:
      kind: sprint
      variable: sprint
    state: dev-start
    stage: dev
    transition: start
    iteration_variable: round
```

`type`, every tag, scope kind, state, stage, transition, and the variable
names are opaque, bounded lower-kebab-case identifiers or bounded tag text.
ATM validates syntax and variable presence only; it does not reserve `dev`,
`qa`, `fix`, `sprint`, or any other workflow value. `iteration_variable` is
optional. A workflow declaration must contain all of `scope`, `state`,
`stage`, and `transition` or be absent; partial declarations fail template
registration before catalog/message mutation.

`metadata.tags` is a duplicate-free literal set. Its stored snapshot and
effective-tag projection use deterministic lexical order. Tags cannot contain
templating expressions. Per-instance workflow variation must use a different
template revision/type or explicit instance tags; dynamic workflow metadata is
not silently inferred from rendered prose.

### 2. Canonical catalog data and immutable message snapshots

`message_templates` stores the parsed declaration and canonical template tags
inside its immutable schema/frontmatter JSON. For a successfully decomposed
admission, `atm-core` resolves and validates the declared scope/iteration
variables from the normal merged-variable value before calling the storage
capability. The one storage transaction persists the already-validated
immutable message snapshot:

- `workflow_scope_kind`, `workflow_scope_id`
- `workflow_state`, `workflow_stage`, `workflow_transition`
- `workflow_iteration` (nullable)
- `applied_template_tags_json`
- `effective_tags_json`

Existing `tags_json` remains the sender/instance tag set. `effective_tags_json`
is a deterministic, duplicate-free union of sender tags, applied template tags,
and automatically derived tags. It is a search projection, not a source of
truth. The canonical workflow snapshot and the two provenance-preserving tag
sets remain queryable independently. Derived tags have no redundant mutable
JSON column: they are reproducible exactly from the immutable workflow
snapshot plus the admitted template type/content format, and are exposed as a
separate derived result set by query projections.

`effective_tags_json` is deliberately materialized even though it is not a
source of truth: it is the canonical local tag-search index/projection. It
avoids reconstructing and unioning three JSON sources in every bounded query,
and permits one deterministic indexed tag-filter path. Admission validates and
writes it atomically from the immutable inputs. Admission construction and
migration/reopen fixture tests recompute the projection and reject a mismatch;
the design has no separate background maintenance verifier. It is therefore
analogous to an FTS projection, not caller-writable business data.
`derived_tags` needs no separate durable column because every source needed to
reproduce it already has an immutable column and it is never the indexed
caller-input surface.

The generated tags use reserved, documented prefixes:

- `template-type:<value>`
- `content-format:<value>`
- `workflow-state:<value>`
- `workflow-stage:<value>`
- `workflow-transition:<value>`
- `workflow-scope-kind:<value>`

ATM rejects caller/template tags using those reserved prefixes; it alone emits
them. The applied snapshot and effective projection are written in the same
transaction as template registration and message admission. Plain, legacy,
and cross-host rendered fallback messages never gain a fabricated workflow
snapshot; their existing sender tags remain valid and their applied/effective
projection is documented as absent/instance-only.

### 3. Query and observability projection

`decomposed_messages` exposes the snapshot columns and all three tag
provenances as an additive, versioned view change. Generic search can filter
the effective tags and exact workflow fields. The local Maturin read-only SQL
surface can compose arbitrary read-only historical queries from the view.

This decision extends the existing sealed `TemplateCatalogStore` / search
contracts authorized by ADR-036; it does not authorize a new optional storage
capability trait. The leaf snapshot/tag DTOs remain in `atm-storage`, the
transport-neutral validation and mapping remain in `atm-core`, and SQLite
remains private to `atm-storage-rusqlite`.

Lifecycle duration and OpenTelemetry work consume a generic pairing request,
not hard-coded process names. A caller supplies the scope kind plus start/end
state or stage/transition selectors; the projection returns ordered durable
facts and can emit an OpenTelemetry-compatible span with the stored message
timestamps and attributes. No live routing, admission, retry, policy, or
security decision may depend on workflow metadata or telemetry.

### 4. Telemetry boundary and failure isolation

`atm-core` owns the sealed, object-safe `WorkflowTelemetrySink` boundary, its
leaf record/error DTOs, and the built-in `NoopWorkflowTelemetrySink`. The
configured implementation and supervised worker are assembled only by
`atm-runtime`; neither CLI nor `atm-http-runtime` may construct an exporter.
AN.11 adds matching machine-readable and Markdown boundary records before
implementation, naming `atm-runtime` as the only allowed *out-of-owner*
implementation/composition site. This is a first-party configuration seam, not
a downstream plug-in API.

```rust
pub trait WorkflowTelemetrySink: crate::boundary::sealed::Sealed + Send + Sync {
    fn emit(
        &self,
        record: WorkflowTelemetryRecord,
    ) -> Pin<Box<dyn Future<Output = Result<(), WorkflowTelemetryError>> + Send + '_>>;
}

pub struct WorkflowTelemetryRecord {
    pub observation: WorkflowTelemetryObservation,
    pub scope_kind: WorkflowScopeKind,
    pub scope_id: WorkflowScopeId,
    pub state: WorkflowState,
    pub stage: WorkflowStage,
    pub transition: WorkflowTransition,
    pub iteration: Option<WorkflowIteration>,
    pub start_message_id: AtmMessageId,
    pub start_timestamp: IsoTimestamp,
    pub end_message_id: Option<AtmMessageId>,
    pub end_timestamp: Option<IsoTimestamp>,
    pub duration_millis: Option<u64>,
}

pub enum WorkflowTelemetryObservation { Completed, Incomplete }

pub enum WorkflowTelemetryError { Unavailable, Rejected, TimedOut }
```

The dyn-safe method has no generic arguments or `Self` return. A single
supervised runtime worker consumes a bounded `mpsc` queue (default capacity
256 records; configured range 1 through 4,096), applies a per-record timeout
(default one second; configuration range one millisecond through 30 seconds),
and does not run on the admission, routing, or request critical path. Producers
use non-blocking `try_send`: when disabled, full, timed out, failed, or shutting
down, the record is dropped with a structured diagnostic/counter and never
retried synchronously. Shutdown closes intake, drains only until its configured
deadline (default two seconds; range one millisecond through 30 seconds), then
cancels the worker and reports unexported records. No detached task survives
runtime shutdown.

Telemetry configuration is parsed and structurally validated at runtime
startup before an exporter is constructed (supported scheme, queue capacity
1–4,096, timeout/drain bounds, credential reference shape, and explicit
payload-redaction mode). Invalid telemetry configuration fails closed to the
no-op sink and is visible in doctor/structured diagnostics; it must not make
mail admission or the daemon serving state depend on a telemetry endpoint.

### 5. Recommended authoring convention

The user guide recommends `<stage>-<transition>` state identifiers, with
lowercase kebab-case stages such as `plan`, `dev`, `fix`, `qa`, and `release`,
and transitions such as `start`, `complete`, `blocked`, `approved`, and
`rejected`. These are examples only. Authors may use another stable convention
as long as a workflow declaration supplies explicit state/stage/transition
fields.

## Consequences

- Historical analytics remain accurate after a template changes, is retired,
  or is edited into a new SHA.
- Catalog joins remain cheap and useful for discovery, but are never required
  to reconstruct an admitted message's classification.
- Search can filter tags without joins while consumers can distinguish template
  classification from sender intent.
- The design introduces additive schema/view fields and narrowly extends the
  existing template catalog/admission capability; it does not create a
  workflow-engine trait or modify the frozen legacy daemon.
- Telemetry export is an opt-in projection of durable history. It must not
  pretend historical messages were live tracing spans at original execution
  time, nor send data off-host without an explicitly configured exporter.

## Rejected alternatives

1. **Catalog join only.** Cheap but wrong for historical audit when a template
   changes or disappears.
2. **One anonymous `tags_json` array.** Loses template versus sender
   provenance and lets user tags impersonate derived lifecycle tags.
3. **ATM-owned `Dev`, `Qa`, and `Fix` enum.** Couples the reusable message
   substrate to one orchestration vocabulary.
4. **Parse state from rendered XML/Markdown.** Recreates the fragile
   file/regex workflow Phase AN was built to retire.

## Required evidence

- Contract tests reject partial/invalid declarations and reserved tag spoofing.
- Migration tests preserve every existing message and template unchanged.
- Admission tests prove exact tag provenance, deterministic effective-tag
  union, same-transaction rollback, and template-revision historical
  stability.
- Query tests prove scope/state/iteration filters and a generic duration/loop
  projection for two unrelated template vocabularies.
- OTel-export tests prove attributes/timestamps are derived solely from the
  stored snapshot; queue-full, timeout, sink-failure, invalid-config, and
  bounded-shutdown tests prove they do not alter routing or message admission.

## Error inventory

| Code | Cause | Caller/runtime recovery |
| --- | --- | --- |
| `ATM_TEMPLATE_WORKFLOW_INVALID` | Partial declaration, invalid opaque identifier/tag, or templating expression in a literal tag | Correct the template metadata and retry; no catalog/message mutation occurred. |
| `ATM_TEMPLATE_WORKFLOW_VALUE_INVALID` | Declared scope/iteration variable is missing, null, non-scalar, empty, or over the bound after normal merged-variable resolution | Supply a valid template variable and retry; no message is admitted. |
| `ATM_TEMPLATE_TAG_RESERVED` | Caller or template attempts an ATM-reserved derived prefix | Remove/rename the tag; ATM generates reserved tags itself. |
| `ATM_WORKFLOW_QUERY_INVALID` | Empty lifecycle selector, unsupported aggregate dimension, invalid bound, or impossible time range | Correct the local query; no storage mutation or telemetry dispatch occurs. |
| `ATM_WORKFLOW_TELEMETRY_CONFIG_INVALID` | Exporter config is malformed or violates queue/timeout/redaction bounds | Runtime selects the no-op sink, reports degraded telemetry in doctor, and keeps mail serving. |
| `ATM_WORKFLOW_TELEMETRY_DROPPED` | Queue full, worker shutdown, per-record timeout, or sink failure | Increment structured diagnostic/counter; do not retry on a mail/request path. Repair exporter/config and inspect diagnostic history. |

AN.9–AN.11 register these codes with the repository's canonical
`docs/atm-error-codes.md` inventory and use typed `AtmError` values at public
validation/query boundaries. Telemetry worker failures are reported through
the structured diagnostic/counter path, not surfaced as a delayed send/read
failure.
