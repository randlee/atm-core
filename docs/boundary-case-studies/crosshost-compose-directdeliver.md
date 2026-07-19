# Case Study: Cross-Host Compose/DirectDeliver Duplication

**Source**: user-mandated 9-point ack/send routing unification checklist, triaged
during the `feature/pAG-s15-othermac-smoke` cross-host smoke session (2026-07-18).
Findings `CROSSHOST-UNIFY-5` and `CROSSHOST-UNIFY-7`,
`.triage/phase-AG/findings/CROSSHOST-UNIFY-5.ttl` and `CROSSHOST-UNIFY-7.ttl`.

## (a) The boundary that was supposed to exist

atm-core/atm-daemon draw a single line between "message semantics" (compose a
mail message, address it, run hooks, persist state) and "transport" (is the
recipient local or on another host, and if remote, which peer endpoint and
port do we use). The intended contract:

- **One** message envelope / handler pair decides *what* a send is (a normal
  compose, an ack reply, a direct delivery of already-composed messages).
- **One** centralized target parser decides *where* it goes: local vs.
  remote, and if remote, which host. That parser's output
  (`ParsedSendTarget` / `RemoteTargetHost`) is the single source of truth
  that downstream transport code should consume, not re-derive.

This is exactly the kind of boundary a trait/abstraction is supposed to
enforce: "session/message layer doesn't know about sockets; transport layer
doesn't re-decide addressing."

## (b) How it leaked

Two independent leaks, both confirmed still open on `feature/pAG-s15-othermac-smoke`
(head `4caa741e`) and its integration target `integrate/phase-AG` (head `e89ad8df`):

### Leak 1 — two message-semantic paths instead of one (CROSSHOST-UNIFY-7, severity: blocking)

The daemon dispatcher (`crates/atm-daemon/src/runtime_health/dispatch_delivery.rs`)
routes sends through two entirely separate handler families that duplicate
the same recipient-resolution and delivery bookkeeping logic:

- `RequestEnvelope::Send(SendRequestEnvelope::Compose(_))` →
  `send_mail_with_runtime_and_post_send_emitter` (`crates/atm-core/src/send/mod.rs:327`,
  invoked from `dispatch_compose_send` in `dispatch_delivery.rs`)
- `RequestEnvelope::Send(SendRequestEnvelope::DirectDeliver(_))` →
  `deliver_direct_messages_with_runtime_and_post_send_emitter`
  (`crates/atm-core/src/direct_delivery.rs:9-10`)

Verified in `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs`:
`DaemonRequestDispatcher::dispatch` pattern-matches
`SendRequestEnvelope::Compose` and `SendRequestEnvelope::DirectDeliver` as
distinct arms, each calling into a distinct core-side function with its own
recipient-resolution and delivery-snapshot logic
(`crates/atm-core/src/send/context.rs::prepare_send_context` for Compose vs.
the direct_delivery module's own context builder). `protocol.rs` carries the
`SendRequestEnvelope::{Compose,DirectDeliver}` enum split at (per the triage
snapshot) lines ~764-774, plus construction sites at ~1102-1129.

The two paths are not a deliberate two-tier design — they grew because
different callers (interactive `atm send` vs. cross-host relay replaying an
already-composed batch) each got their own end-to-end pipeline instead of
sharing one message envelope with a "replay" flag or similar. Every future
change to send semantics (hooks, warnings, ack-intent fields) has to be
applied twice.

### Leak 2 — host-classification reimplemented below the parser (CROSSHOST-UNIFY-5, severity: important)

Target-string parsing genuinely is centralized:
`parse_send_target_impl` in `crates/atm-core/src/send/mod.rs:128-184` (called
from `parse_send_target` / `DefaultSendTargetParser::parse_target`) is the one
place that turns a raw `<agent>@<team>` / `<agent>@<team>.<host>` string plus
an optional `--host` into a `ParsedSendTarget { to, remote_host }`. Storage of
that decision is also centralized (`inbox_message.rs` remote_host
accessors).

But `crates/atm-daemon/src/peer_transport/delivery.rs` does not simply consume
that parsed, normalized fact — it re-derives host classification itself:

- `resolve_remote_port_for_host` (`peer_transport/delivery.rs:318-364`, verified)
  takes the raw `RemoteTargetHost` and re-implements interface/port selection,
  including parsing the host string as an `IpAddr` again
  (`remote_host.as_str().parse::<IpAddr>()`) and matching it against
  `PeerInterfaceRow.bind_addr` / `advertise_addr` to disambiguate multiple
  enabled ports.
- The triage record for `CROSSHOST-UNIFY-5` additionally cites a
  `remote_host_targets_loopback` helper at `peer_transport/delivery.rs:386-394`
  that re-implements loopback detection. That exact function name was not
  found verbatim at HEAD in the `integrate/phase-AG` worktree at the time this
  doc was written — either it has since been renamed/removed, or the citation
  needs re-verification against the branch snapshot the finding was recorded
  against (`4caa741e` / `e89ad8df`). Treat the loopback-duplication claim as
  triage-sourced, not independently re-verified in this pass; the
  `resolve_remote_port_for_host` IP-matching logic is independently verified.

This is a second, independent host-classification layer living below the
parser boundary instead of consuming the parser's output.

## (c) File:line citations (verified unless noted)

| File | Lines | What's there |
|---|---|---|
| `crates/atm-core/src/send/mod.rs` | 128-184 | `parse_send_target_impl` — the one centralized target parser (verified) |
| `crates/atm-core/src/send/mod.rs` | 327 | `send_mail_with_runtime_and_post_send_emitter` — Compose path entry point (verified) |
| `crates/atm-core/src/direct_delivery.rs` | 9-10 | `deliver_direct_messages_with_runtime_and_post_send_emitter` — DirectDeliver path entry point (verified) |
| `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs` | `dispatch`, `dispatch_compose_send` | dispatcher arms that keep Compose/DirectDeliver as two independent pipelines (verified) |
| `crates/atm-core/src/protocol.rs` | ~764-774, ~1102-1129 | `SendRequestEnvelope::{Compose,DirectDeliver}` variant definitions and construction sites (triage-cited; not re-verified line-by-line in this pass) |
| `crates/atm-daemon/src/peer_transport/delivery.rs` | 318-364 | `resolve_remote_port_for_host` — duplicated host/IP-matching logic (verified) |
| `crates/atm-daemon/src/peer_transport/delivery.rs` | 386-394 (per triage) | `remote_host_targets_loopback` — duplicated loopback classification (triage-cited; not found verbatim at current HEAD, needs re-verification) |
| `crates/atm-daemon/src/non_claude_outbound_runtime.rs` | 70-113 | A *third* place with its own `if let Some(remote_host) = request.remote_host` branch that decides local-file-append vs. cross-host relay for the non-Claude outbound / ack-reply delivery path — same local/remote decision, third implementation site (verified) |

## (d) Why this is a boundary leak, not a legitimate cross-boundary need

A legitimate cross-boundary need would look like: transport code asks the
parser/addressing module "is this local or remote, and what's the resolved
endpoint?" and gets back an opaque, already-decided answer it can act on
without re-deriving anything. That is *not* what happens here:

- `resolve_remote_port_for_host` receives a `RemoteTargetHost` (already the
  output of the centralized parser) but then does its own `IpAddr` parsing
  and row-matching to decide *which interface* it maps to — logic that
  belongs entirely inside the addressing/transport-selection module, not
  scattered per call site.
- The Compose/DirectDeliver split is not a transport concern at all; it is a
  message-semantics distinction (interactive send vs. relay-a-batch) that
  leaked into `RequestEnvelope`/dispatcher shape, forcing the daemon to know
  about two message kinds where the trait boundary should have hidden that
  distinction behind one handler with an internal mode.
- The same local/remote decision (`request.remote_host.is_some()`) is made
  independently in at least three places
  (`send/context.rs::prepare_send_context`, `dispatch_delivery.rs::dispatch_compose_send`,
  and `non_claude_outbound_runtime.rs::deliver_payloads`) rather than being
  computed once and threaded through.

The tell: if you have to grep multiple files for "is this remote" logic and
they don't all call the same one function, the boundary that should have
hidden that decision has been bypassed, not extended.

## (e) Recommended fix direction

1. Collapse `Compose` and `DirectDeliver` into one message envelope/handler.
   The interactive-vs-replay distinction should be an internal parameter
   (e.g. a `SendMode` enum) consumed inside one function, not two parallel
   `RequestEnvelope` variants with two parallel core entry points.
2. Make `parse_send_target` / `ParsedSendTarget` the single point that
   produces a fully-resolved local/remote decision. Transport code
   (`peer_transport/delivery.rs`) should consume that resolved value and do
   interface/port *selection* only (which enabled interface serves this
   already-known-remote host), never re-derive "is this remote" or re-parse
   the host string as an IP from scratch.
3. Audit `non_claude_outbound_runtime.rs` and any other outbound path for the
   same `request.remote_host.is_some()` branch; route all of them through the
   same transport-selection call instead of each re-implementing the check.
4. Once collapsed, the daemon dispatcher boundary should have exactly one
   place where "local vs. remote" is decided, and everything downstream
   receives that decision as a fact, not a re-computable input.

See `docs/boundary-case-studies/general-guidelines.md` for the general
checklist this case illustrates (particularly: "two independent
implementations of what should be one decision" and "match/if-let on the
same enum/condition repeated in multiple unrelated call sites").
