# Case Study: Cross-Host Compose/DirectDeliver Duplication

**Source**: user-mandated 9-point ack/send routing unification checklist, triaged
during the `feature/pAG-s15-othermac-smoke` cross-host smoke session (2026-07-18).
Findings `CROSSHOST-UNIFY-5` and `CROSSHOST-UNIFY-7`,
`.triage/phase-AG/findings/CROSSHOST-UNIFY-5.ttl` and `CROSSHOST-UNIFY-7.ttl`.
This is the CROSSHOST-UNIFY triage work from phase-AG.

**Evidence legend**: **verified** = directly re-read from commit/blob
content in this review pass; **triage-sourced** = quoted from TTL
occurrence entries without independent re-read; **approximate** = inferred
from commit diff/history rather than an exact citation.

**Important — branch mismatch, read before relying on citations below**:
this case study's material was drawn from `feature/pAG-s15-othermac-smoke`
(head `4caa741e`) and `integrate/phase-AG` (head `e89ad8df`). It is **not**
re-verified against this branch (`feature/ruthless-boundary-qa-agent`, head
`546106a7`), and the code shape here is different: `protocol.rs` defines
`SendRequestEnvelope::{Compose,Acknowledge}` (no `DirectDeliver` variant),
`crates/atm-daemon/src/runtime_health.rs` and
`crates/atm-daemon/src/peer_transport.rs` are each a single file (not the
`runtime_health/dispatch_delivery.rs` / `peer_transport/delivery.rs` module
layout cited below), and `crates/atm-core/src/direct_delivery.rs` does not
exist at this HEAD at all. Everything below is a **historical snapshot,
triage-sourced from the phase-AG CROSSHOST-UNIFY-5/7 findings** — it
illustrates the boundary-leak pattern this agent should catch, but must not
be read as a live, re-verified claim about the code at this HEAD or as
"confirmed still open" on this branch. Any file:line "verified" label below
means "verified against the phase-AG branch snapshot at triage time," not
"verified at current HEAD."

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

Two independent leaks, documented as open at triage time on
`feature/pAG-s15-othermac-smoke` (head `4caa741e`) and its integration
target `integrate/phase-AG` (head `e89ad8df`). This is a historical
snapshot, triage-sourced from CROSSHOST-UNIFY-5/7 — see the branch-mismatch
note above before treating any of it as current:

### Leak 1 — two message-semantic paths instead of one (CROSSHOST-UNIFY-7, severity: blocking)

The daemon dispatcher (`crates/atm-daemon/src/runtime_health/dispatch_delivery.rs`,
as laid out on the `integrate/phase-AG` snapshot cited above — this module
path does not exist on this branch)
routes sends through two entirely separate handler families that duplicate
the same recipient-resolution and delivery bookkeeping logic:

- `RequestEnvelope::Send(SendRequestEnvelope::Compose(_))` →
  `send_mail_with_runtime_and_post_send_emitter` (`crates/atm-core/src/send/mod.rs:327`,
  invoked from `dispatch_compose_send` in `dispatch_delivery.rs`)
- `RequestEnvelope::Send(SendRequestEnvelope::DirectDeliver(_))` →
  `deliver_direct_messages_with_runtime_and_post_send_emitter`
  (`crates/atm-core/src/direct_delivery.rs:9-10`)

Historical snapshot (triage-sourced, phase-AG CROSSHOST-UNIFY-7), from
`crates/atm-daemon/src/runtime_health/dispatch_delivery.rs` on the
phase-AG branch snapshot: `DaemonRequestDispatcher::dispatch` pattern-matches
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

Target-string parsing genuinely is centralized (triage-sourced, phase-AG
CROSSHOST-UNIFY-5 snapshot; line numbers below are not re-verified against
this branch — at this branch's HEAD, `crates/atm-core/src/send/mod.rs:128-184`
is actually `WarningEntry`, an unrelated struct, and no
`parse_send_target_impl` function was found anywhere in the crate by grep,
so the exact citation needs re-verification against whichever branch this
case study is eventually re-checked on): a single target parser function
(called from `parse_send_target` / `DefaultSendTargetParser::parse_target`
per the triage record) is described as the one place that turns a raw
`<agent>@<team>` / `<agent>@<team>.<host>` string plus an optional `--host`
into a `ParsedSendTarget { to, remote_host }`. Storage of that decision is
also centralized (`inbox_message.rs` remote_host accessors, per the same
triage record).

But `crates/atm-daemon/src/peer_transport/delivery.rs` (phase-AG snapshot;
this branch has a single `peer_transport.rs` file instead) does not simply consume
that parsed, normalized fact — it re-derives host classification itself:

- `resolve_remote_port_for_host` (`peer_transport/delivery.rs:318-364`,
  historical snapshot, triage-sourced phase-AG CROSSHOST-UNIFY-5 — verified
  against the phase-AG branch snapshot at triage time, not re-verified on
  this branch, which has no `peer_transport/delivery.rs` path at all) takes
  the raw `RemoteTargetHost` and re-implements interface/port selection,
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
  `resolve_remote_port_for_host` IP-matching logic is likewise only
  verified against the phase-AG snapshot, not this branch.

This is a second, independent host-classification layer living below the
parser boundary instead of consuming the parser's output, as recorded in
the phase-AG triage snapshot.

## (c) File:line citations (historical snapshot, phase-AG CROSSHOST-UNIFY-5/7 — not re-verified on this branch unless noted)

| File | Lines | What's there |
|---|---|---|
| `crates/atm-core/src/send/mod.rs` | 128-184 (per triage; NOT re-verified — see branch-mismatch note above) | `parse_send_target_impl` — the one centralized target parser (historical snapshot, triage-sourced; at this branch's HEAD, lines 128-184 are actually `WarningEntry`, and no `parse_send_target_impl` function was found by grep in this crate — citation needs re-verification against whichever branch is checked next) |
| `crates/atm-core/src/send/mod.rs` | 327 (per triage) | `send_mail_with_runtime_and_post_send_emitter` — Compose path entry point (historical snapshot, triage-sourced; not re-verified on this branch) |
| `crates/atm-core/src/direct_delivery.rs` | 9-10 (per triage) | `deliver_direct_messages_with_runtime_and_post_send_emitter` — DirectDeliver path entry point (historical snapshot, triage-sourced; this file does not exist at this branch's HEAD) |
| `crates/atm-daemon/src/runtime_health/dispatch_delivery.rs` | `dispatch`, `dispatch_compose_send` | dispatcher arms that keep Compose/DirectDeliver as two independent pipelines (historical snapshot, triage-sourced; this module path does not exist at this branch's HEAD — `runtime_health.rs` is a single file here) |
| `crates/atm-core/src/protocol.rs` | ~764-774, ~1102-1129 | `SendRequestEnvelope::{Compose,DirectDeliver}` variant definitions and construction sites (triage-cited; not re-verified line-by-line in this pass; at this branch's HEAD the enum is `SendRequestEnvelope::{Compose,Acknowledge}` — no `DirectDeliver` variant exists here) |
| `crates/atm-daemon/src/peer_transport/delivery.rs` | 318-364 | `resolve_remote_port_for_host` — duplicated host/IP-matching logic (historical snapshot, triage-sourced; this module path does not exist at this branch's HEAD — `peer_transport.rs` is a single file here) |
| `crates/atm-daemon/src/peer_transport/delivery.rs` | 386-394 (per triage) | `remote_host_targets_loopback` — duplicated loopback classification (triage-cited; not found verbatim at current HEAD, needs re-verification) |
| `crates/atm-daemon/src/non_claude_outbound_runtime.rs` | 70-113 | A *third* place with its own `if let Some(remote_host) = request.remote_host` branch that decides local-file-append vs. cross-host relay for the non-Claude outbound / ack-reply delivery path — same local/remote decision, third implementation site (historical snapshot, triage-sourced; not re-verified on this branch) |

## (d) Why this is a boundary leak, not a legitimate cross-boundary need

(Analysis below is of the historical phase-AG snapshot described in (b); see
the branch-mismatch note at the top before treating any file/function name
as live on this branch.)

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

(Fix direction below was recommended against the historical phase-AG
snapshot; it has not been checked against whether it was ever applied,
partially applied, or superseded on this branch's own Compose/Acknowledge
shape.)

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
