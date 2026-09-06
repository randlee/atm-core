# Member lifecycle transition boundary

`MemberStateTransitionSink` is the replacement runtime's narrow, sealed
notification boundary for a genuine heartbeat transition into `Idle`. The
runtime records the in-memory observation first, drops its mutex guard, and
then invokes the best-effort sink. It owns no storage or receiver implementation.

The active daemon composition supplies `DrainOnTransitionSink`. Its queue
claim is performed on a blocking task, is guarded by the AQ1 delivery-channel
classifier, and dispatches the rebuilt `NudgeKind::Queue` message through the
ordinary receiver selector. A periodic kind-agnostic recovery sweep enumerates
`PendingNudgeStore::list_pending_members` and repeats the same guarded atomic
claim, so missed heartbeats and process restarts do not lose durable nudges.

The transition callback is an optimization; the pending marker and recovery
sweep are authoritative. Herdr members are left to AQ2.7 and bare-CLI members
are handed off by AQ2.5, so neither path is claimed by AQ3.

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
