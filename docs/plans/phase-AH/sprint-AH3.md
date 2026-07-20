---
id: AH.3
title: Hermes Gateway Graft Integration + X-Session-ID Routing
status: planned
branch: feature/pAH-s3-hermes-graft-integration
worktree: ../atm-core-worktrees/feature/pAH-s3-hermes-graft-integration
target: develop
---

# Sprint AH.3 — Hermes Gateway Graft Integration + X-Session-ID Routing

```yaml
plan_type: sprint_plan
phase: AH
sprint: AH.3
worktree: ../atm-core-worktrees/feature/pAH-s3-hermes-graft-integration
branch: feature/pAH-s3-hermes-graft-integration
status: planned
estimated_scope: large
```

## Goal

Wire atm-graft (via the AH.2 Python binding) into the Hermes gateway so
each profile receives ATM nudges as in-process events that route into
persistent named sessions on the gateway.

Add `X-Session-ID` header support to the Hermes webhook adapter so the
bridge process (started alongside the gateway) can translate atm-graft
nudges into HTTP POSTs that target a named Hermes session — not a
per-delivery throwaway session.

This sprint is the first one that bridges atm-core into Hermes. It has two
sides:

- atm-core side: the Python binding (AH.2) is consumed by a bridge process
  that runs alongside the Hermes gateway
- Hermes side: the webhook adapter honors the `X-Session-ID` header for
  persistent session routing, and the Hermes gateway activates one
  atm-graft session per profile on startup that can deliver nudges via the
  same loop (Hermes may choose to embed the Python binding directly in the
  gateway process instead of running a separate bridge; either choice is
  allowed so long as the webhook adapter honors `X-Session-ID`)

## Hard Dependencies

- AH.1 is `PASS` — `session_id` field on the ATM message model is stable
- AH.2 is `PASS` — Python binding is built and loadable
- Hermes Agent 0.17.0+ (gateway + webhook adapter) is the starting point;
  Hermes side changes are owned by the Hermes-side architect (`arch-ctm` as
  designated Hermes integrator)
- Hermes webhook platform must be enabled per-profile on `127.0.0.1`

## Exact Targets

atm-core side (bridge process):

- `crates/atm-hermes-bridge/` (new crate) OR a Python-based bridge script
  bundled into the Hermes install — the implementation choice is the
  Hermes integrator's, the protocol contract is this sprint's
- the bridge process:
  - activates the atm-graft Python binding with a callback
  - on nudge receipt, POSTs to `http://127.0.0.1:<port>/webhooks/atm-nudge`
    with headers:
    - `X-Session-ID: {session_id}` — the durable session_id from the nudge
    - `X-From-Agent: {sender}` — sender identity
    - `X-From-Agent-Display: {sender}:{transport}:{chat-id}@{team}` — the
      display form derived from the sender's `HERMES_SESSION_KEY` +
      `ATM_IDENTITY` + `ATM_TEAM`
    - `X-Request-ID: {message_id}` — idempotency key

Hermes side (webhook adapter):

- `~/.hermes/hermes-agent/gateway/platforms/webhook.py`
- the `/webhooks/{route}` handler honors `X-Session-ID`; when present:
  - the session chat_id is `atm:{from_agent}:{session_id}` (from the
    `X-From-Agent` header) instead of the existing `webhook:{route}:{delivery_id}`
  - the idempotency cache is scoped per session (not per delivery);
    duplicate nudges targeting the same session within a short window are
    de-duped but valid repeat nudges land as separate turns
- new dedicated route `/webhooks/atm-nudge` registered at startup; uses the
  same handler path but with a distinct default HMAC secret configuration
  suitable for loopback-only local delivery

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims.

- bridge process (Rust or Python) that activates the atm-graft Python
  binding and routes nudges to the Hermes webhook endpoint as HTTP POSTs
  with `X-Session-ID` / `X-From-Agent` / `X-From-Agent-Display` /
  `X-Request-ID` headers
- Hermes webhook adapter change honoring `X-Session-ID` to route into a
  persistent named session (`atm:{from_agent}:{session_id}`)
- Hermes-side session routing derives chat_id from the `X-From-Agent` header
  (stripped of any namespace prefix that the sender's session key carries);
  explicit: if display form is `hendrix:telegram:8991600178@hermes`, the
  receiver's Hermes chat_id is `atm:hendrix:telegram:8991600178`
- Hermes-side nudge delivery into the in-session agent loop — Hermes
  receives the HTTP POST and treats it as an inbound user message on the
  target session, running a normal agent turn against that context
- webhook route registration for `/webhooks/atm-nudge` at Hermes gateway
  startup when the atm-graft adapter is configured
- webhook HMAC secret configuration that defaults to `INSECURE_NO_AUTH`
  for loopback-only bind (with a safety rail rejecting same secret on
  non-loopback)
- Hermes-side tests:
  - `X-Session-ID` present → same session reused across 3+ consecutive
    POSTs
  - `X-From-Agent` header is parsed and reflected into chat_id
  - display-form parsing rejects malformed input
  - Hermes Telegram session namespace does not collide with ATM namespace
  - idempotency cache scopes correctly (per-session, not per-delivery)
- bridge process tests:
  - bridge activates atm-graft session on startup
  - bridge routes incoming nudge to Hermes webhook with correct headers
  - bridge logs warn-level tracing when Hermes webhook is unreachable
  - bridge shuts down cleanly when Hermes gateway stops

## Required Work

### Bridge process

The bridge process is the atm-core side of the integration. Either
implementation shape is allowed:

- Shape A: a new Rust crate (e.g., `crates/atm-hermes-bridge/`) that links
  the atm-graft binding via a Python-embed layer (similar to how CPython
  embeds extensions)
- Shape B: a Python script that imports the AH.2 Python binding and runs
  the bridge loop directly

The choice is the Hermes integrator's. The protocol contract is this
sprint's scope. Regardless of shape:

- the bridge runs on a per-profile basis (one instance per Hermes profile
  on the host)
- the bridge uses `ATM_TEAM`, `ATM_IDENTITY`, and `HERMES_SESSION_KEY` from
  its launch environment (supplied by launchd in AH.4)
- the bridge reads its target Hermes profile webhook port from config

### Hermes webhook adapter change

```python
# inside the existing /webhooks/{route} handler
x_session_id = request.headers.get("X-Session-ID")
x_from_agent = request.headers.get("X-From-Agent")

if x_session_id and x_from_agent:
    # persistent ATM session routing
    from_agent_stripped = strip_namespace_prefix(x_from_agent)
    session_chat_id = f"atm:{from_agent_stripped}:{x_session_id}"
    idempotency_scope = "session"  # rather than per-delivery
else:
    # existing per-delivery behavior (unchanged)
    session_chat_id = f"webhook:{route_name}:{delivery_id}"
    idempotency_scope = "delivery"
```

The handler path stays the same; only the session-chat_id derivation and
idempotency cache key change based on the presence of `X-Session-ID` +
`X-From-Agent`.

### Hermes inbound-message dispatch into session

The Hermes webhook adapter must, after computing the session_chat_id, dispatch
the nudge body as an inbound user message on the existing session. The
agent loop processes the message in context. The agent's reply goes via
`atm send` (auto-attached session_id via the Python binding) back to the
original sender.

### Non-Closure

This sprint does not:

- add launchd plists (AH.4)
- validate any end-to-end story (AH.5)
- change Hermes Telegram or Discord channel behavior

## Acceptance Criteria

- the bridge process routes a nudge to Hermes with the correct headers,
  and Hermes creates or finds the persistent session
- three consecutive nudges targeting the same session all land in the same
  Hermes session, preserving conversation context
- a Hermes Telegram session is demonstrably isolated from the ATM session
  created by an ATM nudge targeting the same Hermes profile
- `X-Session-ID` absent falls back to existing per-delivery behavior
  (backwards compatible with existing Hermes webhook users)
- the Hermes webhook platform HMAC default accepts loopback-only binds
  without operator-supplied secret when the atm-nudge route is configured
  via loopback
- no atm-graft Rust source is modified
- no Hermes Telegram/Discord adapter source is modified

## Required Validation

- `cargo build --workspace` (atm-core side)
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- Hermes pytest suite passes (at least the webhook-adapter subset)
- end-to-end nudge round-trip against a running atm-daemon + Hermes gateway
  (no ATM send required; simulated nudge via atm-graft test harness)
- `git diff --check`
