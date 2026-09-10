# Canonical member-state and idle-opportunity boundary

The write-through RAM master roster owns the replacement runtime's one
ephemeral `RuntimeMemberState` per durable member. Authenticated local heartbeat
POSTs (including hook activity) and successful Herdr list polls call the same
typed mutation seam. That mutation atomically records source,
`last_observed_at`, state-edge time, and `RosterStateRevision`; `RuntimeHealth`
is a projection-only reader and owns no member map.

Each accepted `Idle` observation revision publishes one opaque
`IdleOpportunityId` after the roster mutation commits. The publisher owns no
queue/task query or receiver implementation. Failed or incomplete Herdr polls
preserve the prior record and publish nothing. Successful covered unknown or
absent poll entries become `Unknown`, never `Dead`/`Offline`; only an explicit
heartbeat stop maps to `Offline`.

Before Phase AZ, the active composition supplies the retained transition/recovery
adapter. Its queue
claim is performed on a blocking task, is guarded by the AQ1 delivery-channel
classifier, and dispatches the rebuilt `NudgeKind::Queue` message through the
ordinary receiver selector. A periodic kind-agnostic recovery sweep enumerates
`PendingNudgeStore::list_pending_members` and repeats the same guarded atomic
claim, so missed heartbeats and process restarts do not lose durable nudges.

Phase AZ replaces direct transition/drain scheduling with the one attention
selector. The selector consumes an idle opportunity, reserves zero or one
message/task item, and revalidates that the master-roster member remains `Idle`
at the opportunity's exact revision before emission. Neither raw Herdr output,
heartbeat DTOs, nor `RuntimeHealth` snapshots are eligibility authorities.
Bare-CLI behavior remains separate under ADR-054.

The selection boundary carries identifiers only. A durable per-member cursor
alternates queued-message and persistent-task lanes when both are due; task
priority remains local to the persistent lane. The selected queue claim or
task's current assignment attempt is revalidated immediately before a bounded
metadata-only prompt. Scheduler reservations retry the same item and become
permanently failed only after five retryable delivery failures; that terminal
status never closes or consumes the underlying task/message lifecycle.

# Auxiliary observability routes

`GET /v1/health` and `GET /v1/diagnostics` are mounted outside the canonical
`RequestEnvelope` write contract, so they are intentionally absent from
`docs/atm-http-runtime/openapi.yaml` and `HTTP_ROUTE_SPECS`
(`atm_core::api::http_route_surface()`) — `crates/atm/tests/openapi_surface.rs`
enforces that the OpenAPI document matches `HTTP_ROUTE_SPECS` exactly, and
neither auxiliary route belongs to that surface.

Both routes are built in `atm-http-runtime/src/health_route.rs` and
`atm-http-runtime/src/diagnostics_route.rs`, merged together, and wrapped by
`with_auxiliary_admission` (`lib.rs`) before being merged into
`canonical_router`. That wrapper is a single shared load-shed/concurrency-limit
fallback service (`tower::load_shed` + `ConcurrencyLimitLayer`) covering both
routes as one bounded read lane, distinct from the per-route admission layer
applied to canonical write routes — a saturated diagnostics query sheds a
simultaneous health request instead of leaving it unbounded. They are reached
on the same connector (loopback/local Unix socket) as the canonical API, so
they carry the same connection-level trust boundary as the rest of the local
runtime surface; neither route accepts writes.

`GET /v1/diagnostics` is a bounded, read-only projection of the retained
diagnostic timeline (`DiagnosticTimelineStore`):

- Query params: `since`, `until` (unix-ms bounds), `level` (minimum level),
  `component` (prefix filter), `limit` (page size, default
  `DEFAULT_DIAGNOSTICS_LIMIT`, capped at `MAX_DIAGNOSTICS_LIMIT`), `cursor`
  (opaque, from a previous response's `next_cursor`).
- Response: `DiagnosticTimelineRecord` rows plus `truncated` and
  `next_cursor` for keyset pagination.
- Admission: a bounded `Semaphore` of in-flight query workers, held for the
  worker's real lifetime (including a timed-out request whose
  non-cancellable `spawn_blocking` query is still running), plus an overall
  `query_deadline` covering the whole request. Saturation and deadline
  timeouts both return `503 Service Unavailable`.

## Phase AZ received-hook projection

The Tokio runtime forwards the core-built bounded event without reopening or
rendering durable message text. It preserves the metadata-only `title` and
optional `task_id` through Tmux, Herdr, Graft, and queued dispatch selection;
message text is read later through the normal mailbox API.
