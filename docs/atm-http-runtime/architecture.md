# Canonical member-state and nudge boundary

The write-through RAM master roster owns the replacement runtime's one
ephemeral `RuntimeMemberState` per durable member. Authenticated local heartbeat
POSTs (including hook activity) and successful Herdr list polls call the same
typed mutation seam. That mutation atomically records source,
`last_observed_at`, state-edge time, and `RosterStateRevision`; `RuntimeHealth`
is a projection-only reader and owns no member map. Failed or incomplete Herdr
polls preserve the prior record and trigger no nudge. Successful covered
unknown or absent poll entries become `Unknown`, never `Dead`/`Offline`; only
an explicit heartbeat stop maps to `Offline`.

The active composition supplies the retained transition/recovery adapter. Its
queue claim is performed on a blocking task, is guarded by the AQ1
delivery-channel classifier, and dispatches the rebuilt `NudgeKind::Queue`
message through the ordinary receiver selector. A periodic kind-agnostic
recovery sweep enumerates `PendingNudgeStore::list_pending_members` and repeats
the same guarded atomic claim, so missed heartbeats and process restarts do not
lose durable nudges. Bare-CLI behavior remains separate under ADR-054.

Nudge and escalation rules (Phase BA):
1. The nudge path MUST read the exact canonical `RuntimeMemberState` from the
   master-roster record; it MUST NOT read `PickerMemberStatus`, a
   `RuntimeHealth` snapshot, raw Herdr output, or heartbeat DTOs.
2. Heartbeat and Herdr poll ingress MUST update the record only; neither MUST
   inspect queues/tasks or emit a nudge directly.
3. `Idle` with an open task MUST be nudged, no more than once per 60 seconds
   per task.
4. `Active` MUST never be nudged or diverted.
5. `Blocked` or `Offline` MUST escalate once per episode and MUST receive zero
   nudges.
6. The per-agent nudge queue MUST be one ordered list: undischarged
   `atm queue` messages first, then open tasks in
   `(position, assigned_at, task_id)` order; it MUST hold no lane cursor,
   reservation, or fairness state.
7. Escalation MUST be one ordinary message to the roster lead (when exactly
   one) and to every configured escalation recipient, resolved independently;
   at reminder count 10 the runtime MUST escalate once and stop nudging until
   the assignee's state changes or the task is closed.

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
