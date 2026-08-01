# ADR-045 — Runtime Observation Attribution

| Field | Value |
| --- | --- |
| Status | Proposed |
| Scope | Phase AJ runtime observation |
| Relates to | `REQ-CORE-RUNTIME-002`, ADR-015 |

## Decision

Session, pid, heartbeat activity, and derived agent state are in-memory,
best-effort telemetry. They are forbidden inputs to routing, nudge,
notification, retry, admission, delivery, and policy decisions because the
state is neither complete nor proven current.

A successful local command updates telemetry only if `ATM_IDENTITY` and
`ATM_TEAM` are present and agree with any CLI identity/team arguments.
Args-only or mismatched commands retain normal behavior but suppress telemetry;
an info-level diagnostic is allowed. The existing heartbeat ingress remains a
separate telemetry path. Graft may use only its environment-derived caller
context. Roster reload, recovery, transport adapters, peer delivery, and nudge
paths are not telemetry ingress.

An exception requires an explicit requirement, ADR, boundary record, and test.

The existing roster view may render a defined observation for its matching
member. It omits default `Unknown` / absent-session telemetry and never uses
the display state to make a workflow decision.
