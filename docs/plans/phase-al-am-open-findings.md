# Phase AL/AM — Open Finding Task List

Status: mixed — the consolidated review rows identify this PR's resolved plan
changes; the numbered review findings remain open until their stated execution
or decision evidence exists.

This is the authoritative task list for the AL/AM plan-review findings supplied
after commit `4a6fe822`. Original numbering is retained; source numbering has
no findings 2 or 4. All entries are structural until a reviewer explicitly
reclassifies one as wording-only.

## Team-lead consolidated review (round STEP1-R1)

| ID | Severity | Finding | Planned owner | Required closure evidence | Status |
|---|---|---|---|---|---|
| ALAM-TL-B1 | Blocking | AK.11 status and candidate source conflict across the AL plan, AK.11 sprint, and mandate scope. | AL.1 entry gate and AK plan records | All three now state `archived_reference_source`; AL.1 starts from `develop` and uses only archived source `88bca9d5`, without AK completion. | Resolved — archive clarification pending commit |
| ALAM-TL-B2 | Blocking | `MessageReceivedHookEmitter` signature/availability is unverified on the AL baseline and AK.11 candidate source. | AL.1 | AL.1 records exact archived source `88bca9d5`, requires its sealed signature/disposition validation, and copies only the hook-local file set; it forbids inventing a core trait. | Resolved — archive clarification pending commit |
| ALAM-TL-B3 | Blocking | AL.8 combines composition, static proof, multi-adapter/M5 proof, benchmark, and ledger freeze. | AL.8 / new proof sprint | AL.8 now owns composition/static boundaries only; new AL.9 owns physical proof, benchmark, cutover, and ledger freeze. | Resolved in this PR |
| ALAM-TL-B4 | Blocking | Legacy observability/capacity/state-machine removal has no named sprint/negative guard. | AM.1 and AM.5 | AM.1 inventories consumers and classifications; AM.5 owns ledger-confirmed removal, config/doctor/dashboard disposition, and negative guards. | Resolved in this PR |
| ALAM-TL-I1 | Important | AL.7 and AL.8 duplicate the scarce M5 clean-checkout proof without an artifact-reuse policy. | AL.7 and proof sprint | AL.7 produces one SHA-pinned artifact; AL.9 reuses it unless route/client/TLS/composition changes require rerun. | Resolved in this PR |
| ALAM-TL-I2 | Important | Most `must_follow` metadata omits the required merge-forward versus PR-completion trigger. | AL.3–AL.9, AM.1–AM.6 | Sprint metadata now distinguishes pushed-commit merge-forward from deletion PR-completion gates. | Resolved in this PR |
| ALAM-TL-I3 | Important | AM plan summary omits direct AL.8/AM.1 dependencies that AM.3/AM.4 sprint docs require. | `plan-phase-am.md` | Summary and sprint gates now consistently use AL.9, AM.1, and frozen-topology predecessors. | Resolved in this PR |
| ALAM-TL-M1 | Minor | TLS adapter quarantine independence is implicit. | AL.7 | AL.7 explicitly forbids imports from `atm-peer-tls-interop` and `atm-storage/src/tls.rs`. | Resolved in this PR |
| ALAM-TL-M2 | Minor | AL.4 filename implies listener ownership though it is client-only. | AL.4 document | Renamed to `sprint-AL4-shared-client.md`. | Resolved in this PR |
| ALAM-TL-M3 | Minor | AL.1’s unblocks relation does not match AL.3/AL.4 direct gates. | AL.1/AL.2–AL.4 metadata | AL.1 unblocks AL.2 directly; AL.3/AL.4 wait for AL.2. | Resolved in this PR |
| ALAM-TL-M4 | Minor | AL.8 summary says it produces AM ledger though AM.1 owns it. | `plan-phase-al.md` | AL.8 captures reference-graph input; AL.9/AM.1 freeze and own the ledger lifecycle. | Resolved in this PR |

## Blocking

| ID | Finding | Planned owner | Required closure evidence | Status |
|---|---|---|---|---|
| ALAM-F001 | **Deletion ordering may not compile.** AM.2 removes `HttpFrameReader` before AM.3/AM.4 remove legacy local/peer callers; AM.4 may similarly precede replay callers in AM.5. The nominal numbering assumes separability without an actual call graph. | AM.1, then AM.2–AM.5 | `sprint-AM1-removal-ledger.md` deliverable 1 requires a call-graph topology; AM.2–AM.5 state the frozen topology overrides numeric labels. | Resolved — `7e6d97db` |
| ALAM-F003 | **Framework rejection bodies may violate ADR-032.** Axum defaults can return plain text for malformed JSON, oversize body, and header rejection despite the frozen `{code,message}` contract. | AL.1 and AL.2 | AL.1 captures malformed fixtures; AL.2 installs rejection mapping or blocks for a reviewed contract decision. | Resolved — `7e6d97db` |
| ALAM-F005 | **Cutover/rollback is undefined.** AL.5–AL.7 say “move” adapters but do not specify activation, one listener publisher, transition ownership, or regression rollback. | AL.9 | AL.9 deliverable 4 defines the add/activate/retire/owner/rollback table and its one-listener/one-publisher invariant; deliverable 6 parks on failure. | Resolved — `7e6d97db` |

## Major

| ID | Finding | Planned owner | Required closure evidence | Status |
|---|---|---|---|---|
| ALAM-F006 | **Removal-ledger lifecycle is circular.** AM.1 authors a ledger while AL.8 produces it from actual references. | AM.1 and AL.9 | Lifecycle is now AM.1 draft → AL.8 live-reference graph → AL.9/AM.1 freeze → AM.2–AM.5 consume; AL.8 does not freeze the ledger. | Resolved — `7e6d97db` |
| ALAM-F007 | **MessageWriter disposition may be unavailable.** New/idempotent-duplicate/conflict hook semantics assume a tri-state result while the plan forbids unauthorized core-trait expansion. | AL.1 | AL.1 deliverable 7 verifies the surface or explicitly blocks for a core-boundary decision; AL.3 cannot silently add it. | Resolved — `7e6d97db` |
| ALAM-F008 | **Graft write migration has no owner.** The plan requires graft to use the shared client/canonical handler but lacks a named migration sprint and graft smoke. | AL.4, then AL.9/AM.3 gates | AL.4 migrates `atm-graft` off `atm_daemon_client::exchange_request` / `try_connect` to the shared `#[async_trait] DaemonApiClient`; AL.9 and AM.3 cannot proceed without accepted migration; AL.9 includes the graft-path smoke. | Resolved — this plan-fix commit |
| ALAM-F009 | **Benchmark baseline/gate is underspecified.** A post-AL.1 baseline can be contaminated by Cargo feature unification; workload, threshold, environment, and Windows are not closed. | Pre-AL.1 capture and AL.9 | AL.9 deliverable 3 requires the pinned pre-AL SHA, workload/environment/raw artifacts, p50/p99/throughput tolerances, hook-active measurement, and Windows evidence. | Resolved — `7e6d97db` |
| ALAM-F010 | **Hook latency/ADR-041 conflict is unresolved.** In-request hook execution changes sender-observed latency and the ADR conflict is deferred beyond destructive deletion. | AL.3, AL.9, before AM.5 | AL.3 records the ADR-041 interpretation; AL.9 measures hook-active latency; AM.5 blocks deletion until that decision is accepted. | Resolved — `7e6d97db` |
| ALAM-F011 | **Observability, doctor, and configuration parity are unowned.** Deleted transport/replay events, metrics, doctor fields, and strict-config keys lack an inventory/disposition. | AM.1 and AM.5 | AM.1 deliverable 5 inventories consumers/disposition; AM.5 owns ledger-confirmed removal, strict-config disposition, and guards. | Resolved — `7e6d97db` |
| ALAM-F012 | **No abort/rollback or M5 contingency.** AL.8/M5 failure does not define what remains live, and several sprints serialize on an external lane. | AL.9 | AL.9 deliverables 4 and 6 define cutover authority, M5 scheduling, park/rollback, and the AM-start prohibition. | Resolved — `7e6d97db` |

## Minor

| ID | Finding | Planned owner | Required closure evidence | Status |
|---|---|---|---|---|
| ALAM-F013 | **Traceability omits `REQ-DAEMON-TRANSPORT-008`.** AL.5/AL.6 cite it, but the traceability table does not map it. | Traceability record and AL.5/AL.6 | Traceability now has the requirement row with AL.5/AL.6 implementation and proof. | Resolved — `7e6d97db` |
| ALAM-F014 | **Warning representability surfaces too late.** AL.3 blocks on missing existing warning representation, but AL.1 does not make that an acceptance gate. | AL.1 | AL.1 deliverable 8 and acceptance gate make warning representability a start-of-phase decision. | Resolved — `7e6d97db` |
| ALAM-F015 | **Manifest reconciliation count can drift.** “57 baseline manifests” is a number, not a pinned inventory. | Boundary transition inventory / AM.6 | `phase-al-am-baseline-boundary-manifests.md` pins exact paths and baseline SHA; AM.6 records later additions separately. | Resolved — `7e6d97db` |
| ALAM-F016 | **AM.3 and AM.4 order is not mechanically explicit.** Both are non-parallel but no dependency relationship is written. | AM.1 / AM.3 / AM.4 | AM.1 defines the topology; AM.3/AM.4 consume its designated predecessor and reject numeric-order assumptions. | Resolved — `7e6d97db` |
| ALAM-F017 | **No develop-sync policy.** Multi-week integration lacks rebase/merge cadence and conflict ownership despite concurrent phases. | Phase AL integration policy | `plan-phase-al.md` integration-line policy names cadence, owner, merge-forward rule, validation, and AL.9 freeze/reproof behavior. | Resolved — `7e6d97db` |

## Closure protocol

Each finding closes only through a plan PR that updates its owning sprint and
this list with a commit/SHA and direct evidence. The consolidated round above
is implemented by `7e6d97dbc8dedad0e255f668e23de3dbeda1adf6`. A finding that requires an ADR
or API decision remains **Blocked**, not closed, until that decision is accepted
and the dependent sprint is updated. The list is reviewed before AL.1 starts,
before AL.9 activation, and before every AM deletion sprint.

## QA-1 plan-fix batch

| ID | Owner / plan edit | Closure evidence | Status |
|---|---|---|---|
| `RBQA-ALAM-F002` | AL.9 and AM.3 entry gates | Both require AL.4's accepted graft migration before physical proof or local legacy deletion. | Resolved — this plan-fix commit |
| `RBP-F001` | AL.1 lifecycle contract | Consuming `Configured → Running → Draining → Stopped` typestate and negative compile tests. | Resolved — this plan-fix commit |
| `RBP-F002` | AL.1/AL.4 shared-client contract | Existing sealed `DaemonApiClient` moves to `#[async_trait]`; hook trait remains synchronous/object-safe; no blocking bridge or new client trait. | Resolved — this plan-fix commit |
| `RSH-001` | AL.1 startup contract | Typed validation of bind, UDS, limits, timeouts, and TLS before bind/publication. | Resolved — this plan-fix commit |
| `RSH-003` | AL.2 handler | Framework body/in-flight/load-shed bounds with retained ADR-032 overload response. | Resolved — this plan-fix commit |
| `RSH-004` | AL.8 composition | Existing daemon readiness transitions and typed failed-start cause. | Resolved — this plan-fix commit |
| `ARCH-002` | AL.3 hook pseudocode | Post-persistence helper uses non-emitter `record_received_hook_warning_after_persistence` name, satisfying the RULE-002 grep gate. | Resolved — this plan-fix commit |
| `RBP-F003` | AL.4 shared client | Enumerated failure causes and per-stage timeout sources with typed context. | Resolved — this plan-fix commit |
| `RSH-002` | AL.1/AL.4 | Startup validates timeout values; AL.4 bounds DNS, connect, TLS, write, read, and absolute operation. | Resolved — this plan-fix commit |
| `RSH-005` | AL.8 composition | One 5s graceful-drain contract; AL.8 reconciles legacy differing constant at cutover. | Resolved — this plan-fix commit |
| `ATM-QA-004` | Traceability record | `REQ-CORE-TRANSPORT-006` is explicitly historical framing removal, not preserved behavior. | Resolved — this plan-fix commit |
