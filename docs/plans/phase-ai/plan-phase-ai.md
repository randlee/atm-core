---
title: Phase AI Plan — HTTP daemon and minimal cross-host transport
status: proposed
branch: plan/phase-ai-planning
worktree: ../atm-core-worktrees/plan/phase-ai-planning
target: integrate/phase-ai-31-33
---

# Phase AI — HTTP daemon and minimal cross-host transport

## Goal

Provide one production-ready daemon API: Unix HTTP over UDS and loopback TCP,
Windows HTTP over loopback TCP, and HTTPS over TCP for an authenticated remote
daemon. Every ingress reaches
the same read/write handlers. Cross-host code is transport-only.

## Accepted design

| Concern | Decision |
| --- | --- |
| Local client transport | Unix HTTP/UDS plus supported loopback HTTP/TCP; Windows loopback HTTP/TCP only |
| Remote transport | HTTPS/TCP to the same daemon router |
| Public application routes | resource-oriented `/v1/atm/messages`, `/message/{id}`, `/message/{id}/read`, and `/doctor` endpoints; teams routes are an accepted Phase AI waiver |
| Published interface | checked-in OpenAPI 3.1 plus generated JSON; a future web UI is a client |
| Ack | `POST /v1/atm/messages` builds the same write with `acknowledges_message_id`; receiver applies the transition |
| Agent context | Optional `chat-id` is a separately persisted source/destination address component; agent-facing form is `agent:chat-id` |
| Host routing | One post-write router; empty host emits local nudge, every present host uses HTTPS |
| Security | Storage-trait-managed enabled interfaces, local certificate, mTLS, exact trusted peer fingerprint; SQLite is the initial backend |
| Delivery state | No outbox, replay store, retry queue, deferred receipt, or remote ack state; AI.16's disabled-by-default bounded canonical resend scan is the sole exception |
| Offline reconciliation | AI.16 adds an operator-bounded scan of canonical immutable outbound records; it has no cursor, queue, receipt, retry budget, or per-message delivery state |
| Idempotency | Immutable existing message ULID; storage accepts duplicate identity idempotently |

Stable registered hostname plus certificate pin is the peer authority. A direct
IP target is permitted only when fresh DNS resolution of exactly one registered
hostname contains it; DNS results are never persisted and reverse DNS never
creates authority. A remote result is successful only after peer HTTP
acceptance, never after local persistence.

The detailed decisions are ADR-032 through ADR-038. ADR-028 through ADR-031
are historical and superseded.

## Baseline and branch policy

`integrate/phase-AI` starts from `develop`. AI.1 is
`feature/pAI-1-daemon-preag-reset` (PR #592), a clean squash of the reviewed
deletion baseline. The superseded `fix/daemon-pre-ag-deletion-reset` branch is
not a Phase AI target. AI.1 may retain only its singleton/local-IPC baseline
and deletion work; its Phase AG plans, generated gate material, and unrelated
triage changes are not a Phase AI baseline.

`integrate/phase-ai-31-33` branches from `integrate/phase-AI` for AI.31–AI.33
and their AI.39+ follow-up line; completed work merges forward into
`integrate/phase-AI`.

The reset branch is an input, not an authority: every AI.1 deletion must be
validated against fresh `integrate/phase-AI` source and documented here.

## Non-negotiable architecture checks

Each sprint extends and runs the following checks against its own merge base:

1. **One ingress:** exactly one production write handler and one storage write
   call path; no Compose/DirectDeliver or separate ack sender may remain.
2. **One router:** only the post-write router may inspect destination host or
   choose local nudge versus HTTPS delivery. UDS is ingress only.
3. **Transport-only remote adapter:** HTTPS adapter code may authenticate,
   encode/decode HTTP, connect/listen, and call the router. It may not depend
   on SQLite, mailbox mutation, acknowledgement state, nudge sinks, replay,
   receipt, or retry types.
4. **Storage boundary:** HTTP/HTTPS/UDS/loopback-TCP adapters may not use rusqlite or schema
   types; only the sealed storage trait reaches SQLite.
5. **No retired parallel protocol:** source and dependency checks reject the
   custom ATM frame codec and retired Windows local-IPC support
   support after AI.11. Structural
   tests verify the retained router/handler graph, so identifier renaming cannot
   satisfy the gate.
6. **Append-only published surfaces:** CLI and OpenAPI baseline regeneration
   may add entries only. A removal or rename hard-fails even under `--bless`;
   an intentional breaking change requires a separately human-reviewed,
   versioned baseline reset before its implementation PR.
7. **Durable report index:** every producer PR runs `just reports-index
   --check`. It fails on a stale index, malformed/public-unsafe envelope, or
   missing report/evidence link; `site/` follows ADR-044 classification.

Every sprint reports its gate output, changed symbols, required deletions, and
net LOC. A deletion sprint cannot close with a retained target under another
name.

## Shared application contract

All clients use one application request model; transports only translate it:

```rust
pub struct ChatId(/* validated safe segment */);

pub struct AgentIdentity {
    pub agent: AgentName,
    pub chat_id: Option<ChatId>,
}

pub struct AgentAddress {
    pub identity: AgentIdentity,
    pub team: TeamName,
    pub host: Option<HostName>,
}

pub struct WriteRequest {
    pub message_id: MessageId,
    pub caller: AgentAddress,
    pub to: AgentAddress,
    pub body: String,
    pub requires_ack: bool,
    pub acknowledges_message_id: Option<MessageId>,
}

pub trait DaemonApiClient: Send + Sync {
    fn execute(&self, request: ApiRequest) -> Result<ApiResponse, AtmError>;
}

pub struct RequestDeadline(/* monotonic absolute deadline */);

pub struct TrustedPeer {
    pub host: HostName,
    pub https_port: std::num::NonZeroU16,
    pub fingerprint: CertificateFingerprint,
}

pub struct AuthenticatedPeer {
    host: HostName,
    fingerprint: CertificateFingerprint,
    _private: (),
}

pub enum AuthenticatedIngress {
    Local(/* UDS-authenticated local caller */),
    Peer(AuthenticatedPeer),
    UntrustedSmokePeer(/* plaintext-test provenance only */),
}

pub const MAX_HTTP_REQUEST_BODY_BYTES: usize = 1_048_576;
```

`DaemonApiClient` is introduced and owned by AI.6. All boundary traits in this
plan are sealed unless their owning ADR explicitly authorizes an external
implementation; clients consume the API contract but do not create another
ingress trait. `AgentIdentity` owns `agent[:chat-id]` parsing and
`AgentAddress` composes it with team/host and owns full-address parsing and
`Display` rendering; no adapter concatenates address components. AI.6
constructs only the local form
of `AuthenticatedIngress`; AI.9 alone constructs `AuthenticatedPeer` after
mTLS and exact trust validation.

CLI, graft, UDS HTTP, and HTTPS HTTP create or decode this
same request and call one router, handler, storage method, and post-write
event. The sole host decision is in the post-write router: an empty host emits
the local nudge, while every present host—including `localhost` and own IP—uses
HTTPS. A present chat-id
is stored in nullable source/destination columns and rendered to agents as
`agent:chat-id`; it is not a transport session or a second delivery path.
AI.17–AI.21 consume this published contract for the Hermes Python/graft
integration; they do not alter it.

## Sprint sequence

| Sprint | Branch | Single closure |
| --- | --- | --- |
| AI.1 | `feature/pAI-1-daemon-preag-reset` | Rebased minimal singleton/local daemon baseline; peer/replay subsystem is actually gone |
| AI.2 | `feature/pAI-s2-storage-topology` | SQLite is confined to its backend; runtime replay/finalizer escape hatches are gone and retired core boundary records are mechanically rejected |
| AI.3 | `feature/pAI-s3-error-contract-foundation` | One serializable `AtmError` and no protocol error envelope/kind hierarchy |
| AI.4 | `feature/pAI-s4-error-consumer-migration` | All error producers/consumers use the unified contract; direct-construction gate active |
| AI.5 | `feature/pAI-s5-chat-address-identity` | Optional chat-id is stored, parsed, rendered, and preserved as an independent agent context |
| AI.6 | `feature/pAI-s6-http-uds-router` | Initial HTTP/UDS router landing; AI.11 corrects its incomplete resource contract and Windows local-transport closure |
| AI.7 | `feature/pAI-s7-canonical-write-path` | CLI, graft, local UDS, and ack use one canonical write handler and post-write router |
| AI.8 | `feature/pAI-s8-crosshost-control-plane` | Storage/CLI interface, certificate, exact peer-trust control plane, and fail-closed listener startup validation |
| AI.9 | `feature/pAI-s9-https-peer-transport` | mTLS HTTPS peer transport reaches the same router with bounded per-leg timeouts, body limits, and graceful HTTPS draining; no cross-host state subsystem |
| AI.10 | `feature/pAI-s10-crosshost-proof-closeout` | Local, self-IP, two-Mac, and Windows proof matrix plus release readiness |
| AI.11 | `feature/pAI-s11-post-merge-remediation` | Real resource HTTP contract plus Windows loopback-TCP local transport; retired Windows local IPC removed |
| AI.12 | `feature/pAI-s12-post-write-router` | Every write persists before one post-write router selects exactly one nudge or peer delivery action |
| AI.13 | `feature/pAI-s13-peer-smoke-contract` | Reusable peer-pair release smoke runner and evidence contract |
| AI.14 | `feature/pAI-s14-mac-peer-smoke` | Physical Mac↔Mac peer-pair proof |
| AI.15 | `feature/pAI-s15-windows-peer-smoke` | Physical Mac↔Windows peer-pair proof |
| AI.16 | `feature/pAI-s16-offline-reconciliation` | Durable-age-bounded canonical-message reconciliation with no delivery-state subsystem |
| AI.17 | `feature/pAI-s17-hermes-chat-identity` | Client-neutral ambient `ATM_CHAT_ID` resolution, first consumed by Hermes; no schema, CLI flag, or HTTP contract change |
| AI.18 | `feature/pAI-s18-graft-python-bindings` | PyO3/Maturin binding exposes the existing graft client/nudge contract to Python |
| AI.19 | `feature/pAI-s19-hermes-graft-integration` | One typed bridge maps canonical nudge source address to an isolated Hermes `atm:` chat after persistence |
| AI.20 | `feature/pAI-s20-hermes-bridge-deployment` | Per-profile launchd deployment and reproducible bridge runbook |
| AI.21 | `feature/pAI-s21-hermes-closure` | Four Hermes end-to-end stories have retained production evidence |
| AI.21-pre | `feature/pAI-s21pre-crosshost-evidence-harness` | Supported Python/XHTML peer-smoke harness and explicit test-only plaintext wire profile |
| AI.22 | `feature/pAI-s22-loopback-self-send-exemption` | Host-qualified destinations bypass only the unqualified identity self-send guard; advertised-IP is the required same-host TCP proof |
| AI.23 | `feature/pAI-s23-crosshost-shared-write-path` | Local CLI, own-IP, and peer traffic converge at one HTTP `WriteRequest` endpoint, dispatcher, persistence method, and post-write router; release `1.3.2-beta-23` |
| AI.24 | `feature/pAI-s24-host-qualified-ack-receipt` | Advertised-IP host-qualified ACK reply is persisted, readable, and nudged through the canonical peer route; release `1.3.2-beta-24` |
| AI.25 | `feature/pAI-s25-peer-authority-resolution` | DNS-backed hostname/pin peer authority and live trust refresh |
| AI.26 | `feature/pAI-s26-peer-write-deadline` | One propagated peer-write deadline and cancellation contract |
| AI.27 | `feature/pAI-s27-peer-delivery-observability` | Truthful confirmed/unconfirmed delivery result and terminal events |
| AI.28 | `feature/pAI-s28-bounded-peer-recovery` | Backed-off bounded reconciliation after connectivity loss |
| AI.29 | `feature/pAI-s29-crosshost-smoke-rerun` | Receiver-proven Mac↔Windows physical smoke evidence |
| AI.30 | `feature/pAI-s30-semver-http-compatibility` | Schema/HTTP compatibility admission and opt-in SemVer prerelease distribution |
| AI.31 | `feature/pAI-s31-async-local-admission` | SQLite-only local admission response; host-qualified peer work is signalled after response |
| AI.32 | `feature/pAI-s32-independent-peer-jobs` | Bounded non-durable per-ULID peer jobs; no cross-command delivery-order promise or stream abstraction |
| AI.33 | `feature/pAI-s33-admission-capacity-smoke` | Isolated 1,000/s admission proof and ten-run, endpoint-explicit local/cross-host smoke report |
| AI.34 | `fix/hermes-nudge-endpoint-mismatch` | Canonical roster workspace-root resolution for Python-graft post-send nudge endpoint delivery |
| AI.35 | `feature/pAI-s35-graft-root-fallback-observability` | Graft-root fallback observability and operator runbook closure |
| AI.36 | `feature/pAI-s36-graft-receiver-ownership` | One lease-safe receiver owner per canonical graft root/team/agent; crash reclaim and generation-safe endpoint removal |
| AI.37 | `feature/pAI-s37-hermes-recovery-summary` | One ten-second durable-mail-derived recovery summary; no graft mail queue or mailbox mutation |
| AI.38 | `feature/pAI-s38-hermes-steer-nudge-delivery` | Live and recovery graft wake-ups enter the configured Hermes profile through non-interrupting steer, never normal user-message ingress |
| AI.39 | `feature/pAI-s39-buffered-local-http-framing` | Bounded buffered local HTTP request framing for UDS and loopback TCP |
| AI.40 | `feature/pAI-s40-local-transport-benchmark` | AI.33 admission-runner profiles and durable local transport throughput evidence |
| AI.43 | `feature/pAI-s43-remote-https-response-framing` | Buffered remote HTTPS response framing |
| AI.46 | `feature/pAI-s46-reports-index` | Generated durable reports index |
| AI.47 | `feature/pAI-s47-pages-site-home` | GitHub Pages site home and deployment |
| AI.48 | `feature/pAI-s48-fuzz-tooling-port` | Ported `just fuzz` coordinator/probe tooling |
| AI.49 | `feature/pAI-s49-benchmark-report` | Durable benchmark JSON and aggregate HTML report |
| AI.50 | `feature/pAI-s50-fuzz-report` | sc-compose-template fuzz report renderer |
| AI.51 | `feature/pAI-s51-local-http-framing-adversarial-campaign` | First bounded local HTTP framing campaign |
| AI.52 | `feature/pAI-s52-windows-transport-benchmark` | cwin Windows TCP confirmation after accepted M5 performance evidence |

AI.17–AI.21 scope, dependencies, and parallel-execution rules are
authoritative in [plan-ai17-21-hermes-graft.md](plan-ai17-21-hermes-graft.md).
Findings are fixed on their owning sprint before forward merge.

For every remaining Phase AI implementation sprint, the first commit sets the
workspace release for every releasable ATM assembly to the current Phase AI
prerelease plus the sprint number (for example, AI.31 is
`1.4.0-beta-ai.31`). Runtime evidence starts only after `atm doctor --json`
reports matching CLI and daemon release values; release labels are diagnostic,
not protocol admission.

## Verification matrix

| Layer | Required proof |
| --- | --- |
| Unit | error serialization; chat-address parsing/rendering; host normalization; mTLS/allowlist rejection; duplicate ULID write; ack transition |
| Integration | chat-separated inbox/mutation/reply; UDS HTTP read/write/ack; HTTPS router ingress; no local mutation for rejected remote request |
| Smoke | Unix UDS and loopback TCP; Windows loopback TCP; own advertised IP through HTTPS; second Mac bidirectional send/ack; Windows peer participation |
| Durable reports | Producer PR runs `just reports-index --check`; report-index fixtures validate malformed envelopes, stale-index detection, newest-first ordering, links, and ADR-044 public-data classification |
| Regression | `just lint`, `just test`, architecture checks, no retired local transport/custom-frame/peer-replay source remains |

## Explicit non-goals

- remote replay, background retry, deferred delivery receipts, or a durable
  cross-host outbox;
- retired Windows local transport support or fallback;
- a separate remote daemon, separate ack protocol, or cross-host mailbox
  handler;
- inventing a third public verb before a concrete adapter need exists.

## Post-migration retirement inventory

These are mandatory deletions, never fallbacks or compatibility requirements.
AI.6 removes the runtime sources and boundary records when the HTTP/UDS
replacement lands; AI.10 removes the remaining historical documentation after
its final source-consumer check. A closure inventory must name each retained
item and its concrete consumer; an unnamed or renamed survivor blocks closure:

1. `docs/atm-daemon/protocol-icd.md`, the custom-frame codec, and the retired
   `AtmProtocol`/`ClientTransport`/`ServerTransport`/`RequestDispatcher`
   boundary records.
2. Historical ADR-028 through ADR-031, after their sole remaining value has
   been captured by ADR-034/ADR-035 and the project ADR retention policy
   permits archival or deletion.
3. Historical frame/local-transport sections in core, daemon, and CLI architecture
   documents, after the accepted tip has no source or documentation consumer.

The inventory is not permission to retain any item during implementation: the
owning sprint's deletion inventory and architecture gate decide closure.
