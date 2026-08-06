# Phase AL/AM — Requirement and ADR Traceability

Status: binding planning traceability record

This record maps each governing requirement/ADR to one concrete AL/AM action
and proof. It distinguishes active obligations from historical or future
delivery-state text so a migration sprint cannot reintroduce complexity under
the name of compliance.

## Authority rule

The user-approved minimal direct-send design and the accepted current HTTP,
storage, singleton, error, and sealed-boundary ADRs govern AL/AM. Where an
older requirement or proposed ADR describes replay/coordinator behavior, its
disposition is explicitly recorded below; it is not an implementation mandate
for AL/AM.

| Source | Binding implementation detail | Sprint owner | Closure proof |
|---|---|---|---|
| `REQ-CORE-TRANSPORT-001`, `001B`; ADR-033 | Preserve existing route-specific HTTP structs/Serde/OpenAPI; one framework router and injectable typed handler; UDS, loopback, TLS, and in-process adapter use it. | AL.1 records type oracle; AL.2 router; AL.5–AL.7 adapters | JSON/API snapshot comparison, route-to-`ApiRouter` integration trace, all adapter tests |
| `REQ-CORE-TRANSPORT-002`; `REQ-DAEMON-TRANSPORT-001`, `005`; ADR-033 | Local, same-host, and cross-host use the same `POST /messages` body and handler. Connector/authentication differ only before the handler. | AL.2, AL.4–AL.7 | source call graph plus local/localhost/M5 smoke shows one route and handler |
| `REQ-CORE-TRANSPORT-004`; ADR-041 | Remote success means peer canonical-write acceptance; direct failure remains an honest typed failure. No local success is relabelled as remote delivery. | AL.4, AL.7 | success/failure integration tests and retained outcome/event assertion |
| `REQ-CORE-TRANSPORT-005`; `REQ-P-DAEMON-DISPATCHER-001`; `REQ-DAEMON-TRANSPORT-003`, `004` | Reuse the sealed `DaemonApiClient`; one absolute request budget; framework request work is drain-tracked and bounded. | AL.1, AL.4–AL.8 | compile/boundary test, Tokio-time deadline test, shutdown drain/cancel test |
| `REQ-CORE-TRANSPORT-006`; `REQ-DAEMON-TRANSPORT-006`, `007` | Remove custom framing and never introduce UDP. HTTP mechanics belong to maintained Tokio libraries. | AL.1 prevents new code; AM.2 removes legacy code | dependency/symbol negative tests and smoke after deletion |
| `docs/architecture.md` §21.5–§21.6.2 | Preserve thin daemon ownership, strict I/O ownership, injected dispatcher/handler boundary, typed errors, and structured diagnostics. The historical “socket receive loop” description is satisfied by maintained framework code, not retained ATM frame parsing. | AL.1, AL.2, AL.8; AM.2/AM.6 | composition review, source/dependency graph, route/error/observability tests |
| `REQ-CORE-BOUNDARY-001`, `002`; ADR-001 | Reuse sealed core traits and `AtmError`; do not alter `sealed` visibility or implement sealed traits in an unauthorized crate. | AL.1, AL.2 | compile-time boundary checks and error serialization tests |
| ADR-032 | Preserve current HTTP status plus `{code,message}` error JSON. No runtime-private error envelope/catalog. | AL.1 inventory, AL.2 mapper | API snapshot and negative search for alternate error schema |
| ADR-036; `REQ-DAEMON-RUNTIME-002` | Runtime/daemon accept storage traits only; only the approved storage composition layer may know Rusqlite. | AL.1, AL.8, AM.1/AM.6 | Cargo dependency graph and source search for `rusqlite`/concrete storage imports |
| ADR-026; `REQ-P-RUNTIME-001`–`003`; `REQ-DAEMON-RUNTIME-001`, `003` | Preserve existing singleton/launch ownership. No listener starts or endpoint publishes before owner gate; no alternate root/daemon is created. | AL.8 | lifecycle integration test and explicit review of existing owner/endpoint path |
| `REQ-P-RUNTIME-006` | The runtime consumes validated configuration/identity views; it does not recover identity from inherited daemon environment. | AL.1, AL.8 | constructor/config boundary inspection and launch-environment regression test |
| `REQ-CORE-TRANSPORT-002A`, `002B`, `002B1`, `002C`; ADR-040 | TLS/mTLS allowlist and authority policy are connector/authentication work; plaintext-test is untrusted smoke only; same-host remote proof uses ordinary TLS endpoint. | AL.7 | M5 clean-checkout TLS proof; negative authorization and plaintext-profile tests |
| `REQ-CORE-TRANSPORT-005` capacity/body/deadline clauses; ADR-033 | Apply existing documented body, connection, and shutdown limits through framework configuration/middleware. Do not create ATM socket loops or a new scheduler. | AL.4–AL.8 | configuration test, body-limit test, deadline test, shutdown test, benchmark |
| AK.11 accepted `MessageReceivedHookEmitter` semantics | Receive hook is injected, receiver-only, post-new-persistence, duplicate-suppressed, warning-only on failure. `PostSendHookEmitter` has no active runtime reference. | AL.1 transplant gate, AL.3 | three deterministic hook tests and dependency/call-site guard |
| `REQ-CORE-TRANSPORT-003`, `REQ-DAEMON-TRANSPORT-002` | AL/AM create no delivery state, retry, queue, background send, cache, scheduler, or peer coordinator. Direct send is one request/response attempt. | AL.4 onward; AM.5 | failure-path test with task accounting and negative symbol/dependency guard |

## Explicitly deferred or historical material

| Source | Disposition in AL/AM | Guard against accidental implementation |
|---|---|---|
| `REQ-CORE-TRANSPORT-003A` | Historical by its own text; no implementation. | AM.5 removes historical coordinator artifacts. |
| `REQ-CORE-TRANSPORT-003B` and the reconciliation clauses carried by older requirements | Future, separately authorized work only after AL/AM proves minimal direct cross-host behavior. It is **not** an AL/AM feature, test fixture, data model, or compatibility path. | No timer/cursor/queue/batch/replay symbols; future work must start with a new ADR and plan. |
| ADR-041 language that treats hook I/O as detached background work | The authoritative AL hook contract is the AK.11 receiver-only warning behavior. AL.1 records the precise existing warning result and AL.3 uses no detached hook task. Any unresolved wording conflict is documented for ADR reconciliation; no silent semantic change is allowed. | AL.3 blocks if existing route result cannot represent the warning unchanged. |
| ADR-040 | Status is Proposed, so it informs the AL.7 authority/TLS fixture but cannot justify new durable IP, retry, or routing state. | AL.7 keeps authority configuration behind existing approved trait/view. |
| `PeerMessageArray`, peer-only ingress, resend/replay scheduler/coordinator behavior | Dead/retired architecture targeted for deletion, not a compatibility requirement. | AM.2/AM.4/AM.5 deletion ledger and negative guards. |

## PR traceability checklist

Every AL/AM PR must name the rows it touches and attach: (1) the unchanged
public type/schema snapshot, when a route is touched; (2) the exact shared
handler/client call path; (3) the relevant bounded-execution or boundary proof;
and (4) a statement that it introduced no peer-only type, protocol, or replay
mechanism. A PR that cannot name an applicable table row is out of phase scope.
For a boundary change, it must also meet the same-PR artifact rule in
[`phase-al-am-boundary-transition.md`](phase-al-am-boundary-transition.md).
