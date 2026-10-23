# Phase BK extension: virtual tool senders and cron

BK-1–BK-8 have landed on `develop`. BK-9 starts from current `develop`; BK-10–BK-14 consume its checked contract, and BK-15 integrates the completed boundaries. The historical `integrate/phase-bk` branch is not a base for this extension.

| Wave | Sprints | Exit gate |
| --- | --- | --- |
| 1 | BK-9 | Shared contracts and governed compatibility approved |
| 2 | BK-10 SQLite, BK-11 docs/help, BK-12 core, BK-13 cron, BK-14 HTTP | Boundary checks and direct QA; disjoint path ownership |
| 3 | BK-15 | Composed feature tests, executable documentation fixtures, plugin adoption receipts, release validation |

Every new test must drive controlled inputs. Tests that depend on sleeps, races, event-loop ordering or host load are prohibited.

The separate `atm-bk-tool-plugin-adoption` gate inventories every installed physical `tool`/`@tool` plugin and requires a repository-specific registration, rotation, revocation, migration and rollback receipt before BK-15 cutover. Its repository trunk is verified before dispatch.

Reviews: requirements/ADR, architectural boundary, and test-coverage reviews completed; plan-scope review PASS; Trinity critical-plan review PASS. These are planning reviews. Implementation tests run during the respective sprints.

## atm-bk-9: bk-9: tool-cron-contracts

- **Boundary:** shared storage, hook and sender boundary contracts
- **Branch:** `sprint/bk-9-tool-cron-contracts`
- **Initial PR base:** `develop`

**Owned paths**

- `crates/atm-storage/src/contract.rs`
- `crates/atm-storage/src/types.rs`
- `crates/atm-storage/src/address.rs`
- `crates/atm-storage/src/lib.rs`
- `crates/atm-storage/src/testing`
- `crates/atm-core/src/boundary`
- `crates/atm-core/src/hook/request.rs`
- `crates/atm-storage-rusqlite/src/hook_schema.rs`
- `crates/atm-runtime-test-support/src`
- `boundaries/atm-storage`
- `boundaries/atm-storage-rusqlite`
- `boundaries/atm-cron`
- `boundaries/atm-core`
- `boundaries/atm-http-runtime`
- `docs/requirements.md`
- `docs/adr/ADR-066-tool-identity-constant-repo-override.md`
- `docs/adr/ADR-069-atm-cron-schedules-in-daemon-store.md`
- `docs/adr/ADR-068-task-hooks-one-json-contract-daemon-runner.md`
- `docs/adr/ADR-061-governed-interface-schema-versioning.md`
- `docs/atm-error-codes.md`
- `docs/atm-daemon/http-api.md`
- `crates/atm/openapi.yaml`
- `openapi.yaml`
- `docs/atm-rusqlite/boundaries.md`
- `CHANGELOG.md`
- `crates/atm-core/src/hook/cron_contract.rs`
- `crates/atm-core/src/hook/mod.rs`
- `crates/atm-core/src/boundary/cron_hook_runner.rs`

## Goal
Freeze the shared interfaces for virtual tool senders and cron lifecycle/output before parallel implementation.

## Deliverables
1. Amend ADR-066/068/069, requirements and governed schema records; classify CLI, hook, SQLite and HTTP changes under ADR-061, with recorded approval for breaking parts.
2. Add typed storage-neutral sender/cron DTOs, the core-owned `hook/cron_contract.rs` reply types, a separate sealed `CronHookRunner` port, internal non-serializable capabilities, test doubles and boundary manifests. Publish a checked contract test for each changed port.
3. Add additive SQL DDL and versioned hook/HTTP schema declarations plus previous-consumer fixtures where changed. Define typed errors and precise recovery.
4. Freeze exact cron JSON grammar, default/explicit sender precedence, lifecycle template placeholders, persistence/idempotency keys and action generation fence.
5. Inventory installed plug-ins depending on the physical `tool` roster or `@tool` sender and list their repositories in the BK-15 adoption gate.

## This sprint does not close
SQLite implementation, core authorization/parser, cron engine, HTTP transport, installed docs, plug-in installer adoption and composed end-to-end behavior; BK-10 through BK-15 own those.

### Detailed design

## Normative contracts
## Contract and authorization
`tool` is a reserved virtual namespace, not a `TeamName` eligible for ordinary team creation or member administration. `cron@tool` is a built-in daemon principal; plug-in principals are fixed registrations with installer identity and lifecycle ownership. Sender admission must take a trusted capability or daemon-internal context and derive the address there, never trust a supplied `from`/`--as`/ambient variable. Parse and reject arbitrary `@tool` addresses at ingress; distinguish unregistered, reserved, and unauthorized senders with structured errors. Keep historical `@tool` rows readable without making them current credentials.

Cron action context: `HookRunner` distinguishes cron.tick/cron.stop from task hooks. For cron children only, build a clean environment with the approved base keys plus `ATM_IDENTITY=cron`, `ATM_TEAM=tool`, and `ATM_PHASE=<schedule.phase>`; do not inherit caller identity/team. The hook request still carries `team=<schedule.team>` and `created_by`. The current cron dispatcher defaults `as` to the creator; replace that default for *send* actions only. Resolve the effective sender in this order: explicit full-address `from` in the returned send action, existing explicit `as` with `schedule.team` for compatibility, then built-in `cron@tool`. An action with both `from` and `as` is malformed. Explicit ordinary `from` must equal the persisted `created_by@schedule.team` and remain an eligible roster sender. The same rule applies to legacy explicit `as` on cron actions. A different roster member, foreign team, or explicit virtual `@tool` is refused; any future delegation needs its own authenticated creation-time contract and audit. Task-hook action behavior is unchanged. Only the omitted-sender path gets the daemon-internal cron capability. Do not pass that capability to the child or accept ambient variables as proof. Non-send cron actions and task-hook actions retain their existing default/override behavior. The daemon logs the schedule and effective sender for audit. `atm send --from` is a new full-address CLI spelling using the existing mutating-caller authorization, not a way to impersonate another user.

Migration is additive before removal: discover physical tool roster and installed plug-ins, register authorized identities, switch cron and plug-in dispatch, then retire physical roster credentials without deleting messages. Define recovery for registration conflicts and partial installs. The public plugin registration surface must authenticate install/uninstall ownership and must not become a CLI impersonation route. Review storage schema change under ADR-061 if persistence changes.

The contract sprint fixes shared signatures and authorization rules only. BK-10, BK-12, BK-13, BK-14 and BK-15 implement the separate SQLite, core, cron, HTTP and composition boundaries. External plug-in installer adoption is the tracked BK-15 cutover prerequisite.

## Boundary ownership
BK-9 fixes shared contracts. BK-10 owns SQLite persistence; BK-11 owns help/docs; BK-12 owns core authorization, cron reply parsing and template rendering; BK-13 owns cron transitions; BK-14 owns HTTP handoff; BK-15 owns composition and feature-level proof.
## Required typed authorization seam (BK-12 consumer)
Keep the virtual principal separate from ordinary `AgentAddress` parsing. The daemon constructs one non-serializable proof only after cron admission or authenticated plug-in registration; neither CLI JSON, action JSON, environment, repository config, nor peer HTTP body can deserialize or forge it. Contract sketch:

```rust
pub enum ToolPrincipal { Cron, RegisteredPlugin(PluginSenderId) }
pub struct RegisteredToolSender { principal: ToolPrincipal, address: AgentAddress } // private fields; checked constructor only
pub enum SenderClaim {
    Ordinary { address: AgentAddress, caller: CallerContext },
    DaemonTool { sender: RegisteredToolSender, proof: ToolSenderCapability },
}
pub trait ToolSenderRegistry: sealed::Sealed + Send + Sync {
    // The adapter owns the protected trust-root snapshot; request fields alone confer no authority.
    fn apply_signed(&self, request: ToolInstallerRequest, now: IsoTimestamp)
        -> Result<ToolInstallReceipt, AtmError>;
    fn admit_signed_message(&self, proof: ToolRuntimeProof, message_id: AtmMessageId,
        digest: MessageDigest, now: IsoTimestamp) -> Result<RegisteredToolSender, AtmError>;
    fn resolve(&self, sender: PluginSenderId) -> Result<RegisteredToolSender, AtmError>;
}
```

`ToolSenderCapability` is internal to trusted daemon composition and not serializable or publicly constructible. `PluginSenderId`, `InstallerId`, `MessageDigest` and `ToolNonce` are validated newtypes. The SQLite registry adapter is constructed with the BK-15 loaded protected trust-root snapshot; `apply_signed` verifies the canonical signature, key/host/sender allowlist, expected generation, expiry and nonce *inside the registration transaction*, then mints `TrustedInstaller` internally and applies register, rotate or uninstall. `admit_signed_message` verifies the active runtime key and binds its nonce to `(message_id,digest,sender,generation)` in the same durable ledger. An exact retry returns the same admission receipt; a changed digest/ID with a consumed nonce is refused. BK-14 then calls the canonical writer with that stable `message_id`; an uncertain write may be retried idempotently with the same proof/ID/body. `RegisteredToolSender` fields remain private and `resolve` alone cannot mint an emission capability. `ToolPrincipal::Cron` is built in, never a registry row and never removable by uninstall. The sealed port is implemented only by approved SQLite/test adapters under ADR-001.


For cron `send` action resolution: explicit full-address `from` (ordinary, authorized roster sender) wins; else legacy same-team `as`; else daemon-owned `cron@tool`. Reject `from`+`as`, foreign team and any explicit `@tool` claim before dispatch. The omitted-sender path must be bound to a specific cron schedule invocation and its creator, so BK-15 can route custom output to the persisted creator without letting module JSON choose a `@tool` sender or recipient. Preserve the schedule's real team for actions and storage even though the child defaults to `ATM_IDENTITY=cron`, `ATM_TEAM=tool`. Cross-host ingress must carry server-verifiable provenance, not a serialized capability object.

## Existing caller-context and governed-interface reconciliation
`docs/requirements.md` §4.1-4.2 currently disallows `atm send --from`, while §6.2-6.3 still lists an old `--from <name>` sender override. Rewrite both sections in one change: full-address `--from <agent>@<team>` is an explicit *caller-context spelling* for `atm send`, never an impersonation token. It wins over ambient defaults only when the invoking shell's authenticated base agent equals `<agent>` and the resolved caller team equals `<team>`; if `--team` is present it must match. `--from` is mutually exclusive with `--as` and `--chat-id`; existing chat-qualified `--as` semantics and `ATM_CHAT_ID` precedence stay unchanged. An unqualified legacy `--from <name>` is accepted only as a documented same-team compatibility alias if currently supported by a shipped CLI; otherwise it fails with recovery to `--as <name>` or qualified `--from`. Record the observed shipped behavior before finalizing the compatibility branch. `atm list/read --from` remain filters. Cron reply JSON `from` is a different, daemon-authenticated action field and cannot be copied into a CLI request as authority.

Registry persistence, old physical-tool retirement, explicit `@tool` rejection, new CLI sender syntax, and any cross-host wire field are governed by ADR-061 D2-D3. Classify each separately; additive registration table and optional wire field require version/documentation and previous-consumer fixtures. A breaking validation or retired physical credential requires Rand's recorded major-change approval and a coexistence/negotiation window before cutover. Do not claim the old ADR-061 D6 record approves the new registry schema. Own the relevant OpenAPI copies, HTTP baseline and storage schema record; obtain schema approval before implementation. An authenticated peer connection proves a host, not a tool principal: receiver must verify a host-bound sender registration/capability asserted by the origin and refuse an untrusted peer's bare `@tool` string.

Tool principal registration authorizes ordinary mail emission only in this sprint. It does not make `atm-bd@tool` a cross-team task owner or grant task assignment; ADR-066 D5's lead-mediated task assignment remains until a separate product decision/ADR specifies task ownership and authorization. All task ingress paths still reject spoofed `@tool` claims.

## Deterministic coverage review and implementation gate
Existing cron tests exercise expression parsing, worker scheduling, jitter, stale-generation storage writes, and action dispatch. They do not establish virtual sender authorization. Add table-driven tests with an in-memory registry and fixed authenticated request contexts covering: missing/duplicate/foreign plug-in registration; same-owner reinstall; uninstall isolation; restart and migration reruns; preserved historical mail; ordinary `tool` roster/pane/team creation rejection; `ATM_TEAM`/`ATM_IDENTITY` and JSON/CLI `--as` spoofing; local and HTTP/cross-host sender claims (including a trusted host carrying an untrusted tool claim). Capture both cron.tick and cron.stop child environments and assert task-hook environment remains unchanged. For action JSON, assert omitted sender uses `cron@tool`, authenticated persisted-creator `from` wins, a different sender and mixed `as`+`from` fail before write, and module-provided virtual sender cannot bypass the internal capability. Exercise every documented `atm send --from`/`--as` example through the real Clap parser and caller-context resolver; verify the advertised success and refusal codes. Use fixture stores and explicit request contexts, never sleeps or live host timing.

## Cron contract
## Architecture and ownership
BK-9 specifies the daemon-owned `cron@tool` sender capability. BK-12/BK-15 consume that internal capability; module JSON and `ATM_IDENTITY`/`ATM_TEAM` never carry it. `atm-storage` owns wire-neutral result/output/intent types (`CronDiagnosticDetail`, `CronOutputBody`, and `CronModuleOutput` included) and the sealed persistence port; `atm-storage-rusqlite` owns the SQLite transaction and replay query; `atm-core::hook` owns parsing and conversion into those storage-neutral types; `atm-cron` classifies results and commits transitions; `atm-core::send::nudge_template` owns event rendering; `atm-http-runtime`/daemon bootstrap own nudge and ordinary-mail handoff. No `atm-cron -> atm-http-runtime` dependency or direct SQL in `atm-cron`. Treat ADR-001's seal as a workspace convention: register only the SQLite and test-double implementations in boundary manifests. Use `CronScheduleId`, `CronGeneration`, `AgentName`, `TeamName`, `IsoTimestamp`, and `AtmErrorCode`, not raw strings or integers at crate boundaries.

## Typed state and persistence seam (contract sketch)
Define these types in `atm-storage` and re-export through `atm-core::boundary`; never import `atm-core` from `atm-storage` (that would create a crate cycle). The exact Rust module names may follow existing re-exports, but preserve the signatures and invariants:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CronFailureKind { Timeout, MalformedReply, RunnerExit, Refused, ActionFailed, StopCheckFailed }

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CronFailure {
    pub kind: CronFailureKind,
    pub code: Option<AtmErrorCode>,
    pub detail: CronDiagnosticDetail, // bounded and redacted; not module mail content
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronRunTransition {
    Success { outcome: CronSuccess },
    StopCheckContinues, // output intents only; no last_outcome/last_run_at update
    Failure(CronFailure),
    Stopped { reason: CronDiagnosticDetail },
    SkippedInFlight, // no success reset and no lifecycle intent
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct CronNoticeKey {
    pub schedule_id: CronScheduleId,
    pub generation: CronGeneration,
    pub transition_seq: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CronLifecycleEvent {
    Error { failure: CronFailure },
    Stopped { message: CronDiagnosticDetail },
}

pub struct CronNoticeIntent {
    pub key: CronNoticeKey,
    pub team: TeamName,
    pub recipient: AgentName, // copied from persisted created_by
    pub name: CronName, // parse persisted String at this transition boundary
    pub event: CronLifecycleEvent,
}

pub struct CronTransitionWrite {
    pub id: CronScheduleId,
    pub generation: CronGeneration,
    pub at: IsoTimestamp,
    pub transition: CronRunTransition,
    pub outputs: Vec<CronModuleOutput>,
}

pub trait CronTransitionStore: sealed::Sealed + Send + Sync {
    fn commit(&self, write: CronTransitionWrite) -> Result<CronCommitResult, AtmError>;
    fn pending(&self, limit: usize) -> Result<Vec<CronDeliveryIntent>, AtmError>;
    fn mark_delivered(&self, key: CronDeliveryKey, receipt: DeliveryReceipt)
        -> Result<bool, AtmError>;
}
```

`CronCommitResult` distinguishes `Applied { intents }` from `StaleGeneration`; a stale generation changes neither `last_outcome` nor an intent. `CronDeliveryIntent` is a sum type of `Lifecycle(CronNoticeIntent)` and `ModuleMail(CronModuleMailIntent)`, each with a stable idempotency key. `CronSuccess` is typed (`Ok` or `Actions { count }`), so the existing `last_outcome` strings remain a projection, not the control state. `CronDiagnosticDetail` redacts runner secrets and strips volatile diagnostics (timestamps, elapsed durations, transient paths/ids) before persistence. `CronOutputBody` only validates UTF-8, non-empty content and byte length; the module-authored body is persisted verbatim as normal mail, without silent redaction or rewriting. They are distinct newtypes so the two policies cannot be confused. `CronFailure::fingerprint()` yields `CronFailureKey { kind, stable_code, stable_reason }`: timeout/malformed/exit/action/stop-check use stable codes and category-specific reasons, never stderr tails or elapsed time; refused uses a normalized bounded refusal reason, with variable diagnostic context excluded. Equality compares the full typed key (or collision-checked canonical bytes), not a display string or hash alone.

In one SQLite transaction, condition on `(id, generation, active)` and update the existing `last_run_at`, `last_outcome`, `active`, `ended_at` fields plus a failure episode/last-notified fingerprint, monotonic transition sequence, and delivery intents. New rows/tables and indexes are additive under ADR-061, with a previous-consumer proof. `Success` resets failure suppression; first failure or a different fingerprint creates one error intent; identical failure does not. `Stopped` creates one stopped intent and deactivates the row; a repeated stop cannot create another. `SkippedInFlight` neither clears nor creates. Restarting an ended name increments generation and resets the episode. Manual `atm cron end` is also a normal stop: atomically deactivate each active matching row and insert one stopped intent per row, with a stable reason such as `ended by <authenticated caller>`. Repeated end creates none. Document this alongside until and stop-hook stops.

A delivery worker drains pending intents with bounded batches and explicit retry state. Exactly one *logical durable intent* is guaranteed per transition; exactly one external nudge is guaranteed only if the receiving handoff durably deduplicates `CronNoticeKey`. Lifecycle delivery uses the recipient nudge/handoff transport only, with no `mail_message_states`, task, or ack row. The handoff must accept `CronNoticeKey` as its idempotency key or durably deduplicate at the receiving boundary. If the current transport cannot distinguish an accepted handoff from a failed one, it must expose an `accepted/definitely-not-accepted/unknown` result and retain unknown intents for reconciliation; do not claim exactly-once from an unacknowledged external side effect. Module mail uses a stable `AtmMessageId` derived/stored per output intent and ordinary canonical write admission; replay compares equal and cannot create a second mail row. A missing/deleted creator is a typed terminal delivery outcome retained for operator inspection, never an alternate recipient. A template disabled for the schedule team marks the lifecycle intent suppressed; it does not create mail.

## Cron module reply contract
Task-hook `HookReply` and its exact-key parser remain unchanged. Add a cron-only parser (e.g. `CronTickReply`/`CronStopReply`) in `atm-core::hook`, selected by `HookName::CRON_TICK` or `CRON_STOP` in the runner. It composes the existing action result instead of widening ordinary task-hook replies:

```rust
pub enum CronTickReply {
    PassThrough { output: Vec<CronModuleOutput> },
    Handled { actions: Vec<HookAction>, output: Vec<CronModuleOutput> },
    NotApplicable,
    Refused { message: CronDiagnosticDetail },
}
pub struct CronStopReply {
    pub stop: bool,
    pub reason: Option<CronDiagnosticDetail>, // required iff stop=true
    pub output: Vec<CronModuleOutput>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CronModuleOutput {
    Message { body: CronOutputBody },
    Error { body: CronOutputBody },
}
```

Wire examples (one JSON object on stdout; unknown/duplicate keys, extra stdout, empty body, oversized array/body are malformed):

```json
{"result":"pass-through","cron-output":[{"kind":"message","body":"build ready"}]}
{"result":"handled","atm-action":[{"command":"send","request":{"...":"existing WriteRequest body"}}],"cron-output":[{"kind":"error","body":"upstream API degraded"}]}
{"stop":true,"reason":"phase complete","cron-output":[{"kind":"message","body":"final report available"}]}
{"stop":false,"cron-output":[]}
```

`cron-output` is optional (omission means empty) and capped at 32 entries, each body at the ordinary send inline limit or a lower explicitly documented bound. Use a cron-only streaming/typed `serde` deserializer that rejects duplicate keys at the top level and inside each output/action object; parsing through `serde_json::Value` alone cannot detect duplicates. It is accepted on pass-through, handled, and both stop replies; it is rejected on refused/not-applicable. A custom `kind=error` is *content*, not a scheduler failure. `--custom` remains input passed to the module; no new `atm cron start` output flag or schedule column is needed. Accepted outputs persist as ordinary ATM mail, with `from=cron@tool`, `to=<created_by>@<schedule.team>`, `requires_ack=false`, no task id, and classification `category="cron"`, tags `schedule:<name>` and `output:message|error`, `content_format="text/plain"`. Mail body is the module's validated body verbatim; summary is `cron <name>: message` or `cron <name>: error`. The classification and sender are set by the daemon, not copied from module JSON. This is the persisted and CLI-visible general custom-message contract (`atm read`, `atm list`, `atm search`); no private output store becomes a second mailbox.

Parse and validate the entire reply before effects. For `handled`, dispatch existing actions in order under BK-9 sender rules; on action failure, record `ActionFailed` and do not enqueue custom outputs. Earlier actions may have committed per ADR-068 and are reported in structured error context. A store failure after action dispatch cannot roll those actions back; document this as an existing partial-commit boundary and require idempotency/replay keys for actions that must survive cron retries without repeating effects. The lifecycle intent guarantee covers result/notification state, not arbitrary external action side effects. On successful actions or pass-through, commit result and output intents together. For `cron.stop`, commit output intents with either a successful non-stop check or the stopped transition; drain output intents before the corresponding lifecycle nudge for a single transition. Delivery failure of an already committed custom-mail intent is retried as delivery state; it does not retroactively change a successful module result.

## Lifecycle template contract
Add `BuiltInNudgeTemplateKind::Cron` with serialized value `cron`, parsed by the same override store and exposed by the existing `atm teams` nudge-template commands for the schedule's real team. Keep the single `team_nudge_template_overrides` row key `(team_name, template_kind)`; do not create separate error/stopped categories. The default body is:

```text
<atm cron name="{{name}}" event="{{event}}" {{detail_attribute}}>
```

The renderer provides only `name`, `event`, and `detail_attribute` to a `cron` template. The nudge is ATM protocol markup (XML-like, not an XML document). It constructs `detail_attribute` itself as exactly `message="..."` for `Stopped` and `error="..."` for `Error`; values are escaped for XML attributes (`&`, `<`, `>`, `"`, `'`), normalized for control characters, and bounded. `event` is the fixed literal `stopped` or `error`. A module cannot supply these fields. For example:

```text
<atm cron name="nightly" event="stopped" message="phase complete">
<atm cron name="nightly" event="error" error="timeout: ATM_HOOK_TIMEOUT">
```

At the `atm teams` set boundary, dispatch validation by `BuiltInNudgeTemplateKind`: the existing generic validator remains for post-send kinds, while `cron` uses its own placeholder set and renderer and never fabricates a `PostSendHookEvent`. A per-team override for kind `cron` may use `{{name}}`, `{{event}}`, `{{detail_attribute}}` only. Reject unsupported placeholders and conditional syntax at set time; render these three with the cron-specific validator, not the post-send `PostSendHookEvent` value map. Even inside an override, `detail_attribute` is built by trusted code and never accepts raw markup. Disable suppresses both lifecycle event variants, not custom mailbox output. Default and override delivery is a nudge, with no `<action>atm read`, no message id, and no ack. Record an operator-visible reason if rendering or delivery fails, while retaining the intent for safe retry.

## Typed errors and recovery
Use existing `AtmErrorCode` values when they precisely describe a module timeout/malformed reply/action failure; add stable cron-specific codes only for new states (e.g. malformed custom output, creator unavailable, lifecycle delivery uncertain) and update `docs/atm-error-codes.md`. Every error records schedule id/generation, phase/team, failure kind, cause, and recovery action without logging raw secrets. Verify with fixed clocks, controlled runner/dispatcher, forced storage/handoff failures, and restart replay. Include a crash injection after an accepted handoff but before `mark_delivered`, a duplicate-key JSON fixture, and a store-failure-after-actions fixture. No sleeps, races, event-loop ordering, or load-sensitive assertions.

## Runner and delivery port details
The current sealed `HookRunner::run` returns `HookOutcome { reply: HookReply }`; it cannot carry cron output as written. Add a separate object-safe sealed `CronHookRunner::run_cron(&self, target: &HookTarget, request: &HookRequest, deadline: RequestDeadline) -> Result<CronHookOutcome, AtmError>` port. It invokes the cron-only parser in its BK-12 implementation. Keep existing `HookRunner::run` and `HookReply::parse` for task hooks unchanged. Extend the cron-only `stop` return with `output` (or return `CronStopReply`) without allowing cron output from `doctor`. The implementation and `DummyHookRunner` must expose controlled results for both paths; `atm-cron::tick::run_one` calls `run_cron` instead of `run`. The new `CronHookRunner` port follows ADR-001 workspace-convention sealing; BK-9 adds its typed test double and boundary manifest entry, while BK-12 implements parsing and BK-15 implements the production adapter. Do not change `pub mod sealed` visibility.

```rust
pub struct CronHookOutcome {
    pub reply: CronTickReply,
    pub elapsed_ms: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronSuccess { Ok, Actions { count: usize } }
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CronDeliveryKey {
    Lifecycle(CronNoticeKey),
    ModuleMail { schedule_id: CronScheduleId, generation: CronGeneration,
                 transition_seq: u64, output_index: u16 },
}
pub struct CronModuleMailIntent {
    pub key: CronDeliveryKey,
    pub message_id: AtmMessageId,
    pub team: TeamName,
    pub recipient: AgentName,
    pub name: CronName, // parse persisted String at this transition boundary
    pub output: CronModuleOutput,
}
pub enum CronCommitResult { Applied { intents: Vec<CronDeliveryIntent> }, StaleGeneration }
pub enum DeliveryReceipt { Accepted { receipt_id: String }, Suppressed, RecipientUnavailable }

#[async_trait]
pub trait CronLifecycleNudger: sealed::Sealed + Send + Sync {
    async fn deliver(&self, notice: &CronNoticeIntent) -> Result<DeliveryReceipt, AtmError>;
}
```

The `CronLifecycleNudger` implementation lives with the existing daemon nudge dispatch adapter, not in storage or `atm-cron`. Its `deliver` method must be idempotent on `notice.key`; if the existing adapter lacks this property, add a narrow idempotent handoff adapter there. `AtmCronDeps` receives the transition-store and delivery ports from `atm-daemon-bootstrap` composition; the `atm-cron` crate does not construct concrete SQLite, HTTP, pane, or Herdr transports. For module mail, the canonical writer consumes `CronModuleMailIntent` with its preallocated `message_id` and BK-9 capability; a caller cannot set either from reply JSON. Delivery result and any retry cause are persisted through `mark_delivered`; `Unknown` handoff does not mark success or generate a second logical transition; it remains in a reconciliation state and is not blindly resent until the receiver can query or deduplicate the same key.

## Governed version and template inventory
`HOOK_SCHEMA_VERSION` is presently `1.0.0`. An optional cron-only `cron-output` field is an additive candidate for `1.1.0`; old cron replies with no field remain valid, and ordinary task-hook shapes remain exact. Classify duplicate-key rejection separately as a validation change under ADR-061/ADR-068 D8; if schema review classifies it major, obtain Rand's explicit recorded approval and a coexistence plan before merging. Keep HTTP/peer versions unchanged unless a new wire field is actually added; then update both OpenAPI copies, surface baseline, version record and previous-consumer fixture. The additive SQLite intent schema also requires its own ADR-061 approval/version record and previous-binary fixture; ADR-061 D6's old cron_schedule approval is not reused.

The post-send path continues to have exactly eleven built-in cases. `cron` is a twelfth *stored override kind* in the same table, but it is a separate lifecycle nudge family selected by `CronLifecycleEvent`, not `PostSendHookEvent`. Amend the two existing requirement clauses that say 'eleven' to state this distinction explicitly, along with REQ-P-CRON-005 and the operator template documentation. No external `[[atm.post_send_hooks]]` intercepts the cron lifecycle nudge unless separately approved.

A `cron.stop` reply with `stop=false` and output is committed as a `StopCheckContinues` observation with module-mail intents and a new transition sequence *before* calling `cron.tick`. It leaves `last_run_at`, `last_outcome`, and failure suppression unchanged; a subsequent tick failure does not retract the earlier custom mail. The check result and tick each recheck the generation. A `stop=true` reply commits the custom mail intents and stopped lifecycle intent in one transaction, and output is handed off first. On manual `atm cron end`, any in-flight action may already have committed, but no new action may be admitted after the end transaction's linearization point. The dispatcher needs a generation-scoped `CronRunPermit` checked at each action's canonical write/admission boundary; a pre-dispatch read alone is insufficient. If an external action cannot be fenced atomically, report it as a documented partial-commit outcome and do not falsely promise rollback.

## Additional boundary types
```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CronFailureKey { pub kind: CronFailureKind, pub stable_code: Option<AtmErrorCode>, pub stable_reason: String }
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CronDeliveryIntent { Lifecycle(CronNoticeIntent), ModuleMail(CronModuleMailIntent) }
pub struct CronRunPermit { schedule_id: CronScheduleId, generation: CronGeneration, nonce: OpaqueNonce }
```
`CronRunPermit` is minted only by the scheduler, is never serialized into module JSON, and is checked in the canonical write/admission transaction for every cron action. The dispatcher carries it as an internal optional field; task-hook actions have none. A stale permit fails before a new side effect. The permit cannot roll back an action already committed before a concurrent end/restart.

## Deterministic coverage review and implementation gate
Current `atm-cron` worker/storage tests cover manual-clock ticks, stop-check true/false, timeout and runner errors, stale generation, and restart; current reply tests cover task-hook shapes. None prove custom output, lifecycle nudge count, durable retry, sender authentication, or the cron-only duplicate-key rule. Add a case matrix with controlled clock, runner, dispatcher, SQLite fault injection and handoff receipts: first/repeated/changed failure key; success reset; skipped in-flight does not reset; until, stop-hook and manual end each create one stopped intent; repeated end and stale generation create zero; name reuse increments generation; stop=false output commits before a later failing tick without changing `last_outcome`; stop=true output hands off before its nudge; `kind=error` output is ordinary mail and does not fail a successful run. Cover pass-through/handled, action failure after earlier committed actions, malformed/duplicate/unknown keys at every JSON nesting level, empty/oversized/multiple outputs, refused/not-applicable output rejection, and task-hook parser regression. Assert exact sender, persisted creator recipient, category/tags, unread/mailbox presence, absence of lifecycle mail/ack/task rows, and HTML/XML-attribute escaping for each nudge detail variant and override. Inject commit failure, handoff rejected/unknown/accepted-before-mark, restart replay, missing creator, disabled template, old SQLite/old hook-schema consumer and local/cross-host idempotency; assert one logical intent per transition and no claim of external exactly once without receiver dedup. Pause action admission with a controlled barrier to prove end/restart prevents the *next* action while already-committed actions remain. Drive state by explicit steps and receipts, not sleeps, races, event-loop order, or load.

## Contract handoff
`atm-storage` owns wire-neutral DTOs and the sealed storage port; `atm-core::boundary` re-exports it and owns hook runner/dispatcher/nudger signatures. The contract test doubles are the only permitted untrusted implementations. All later sprints consume these names and behavior. This bead fixes interface shape, not production implementation. The architecture dependency is storage -> core -> cron/HTTP -> daemon composition, with no reverse import or direct SQL outside SQLite.

## External plug-in inventory gate
Before contract sanity closes, inventory every installed sender plug-in and repository that currently depends on a physical `tool` team or emits an `@tool` sender. Record the exact list (including `randlee/atm-bd`, or an explicit none for any other plug-in) in the adoption bead. The inventory is the cutover set; BK-15 waits for one linked implementation/release/compatibility receipt per row. Newly discovered senders are added before cutover, not silently ignored.

## Frozen cron reply contract file and compile handoff
`crates/atm-core/src/hook/cron_contract.rs` is owned by BK-9 and exports `CronTickReply`, `CronStopReply`, `CronHookOutcome`, and the cron-only reply version/limits. `crates/atm-storage/src/types.rs` owns `CronModuleOutput`, `CronOutputBody`, `CronDiagnosticDetail` and persistence DTOs, re-exported through `atm-core::boundary`; there is no storage-to-core import. `crates/atm-core/src/hook/mod.rs` re-exports the contract types. `crates/atm-core/src/boundary/cron_hook_runner.rs` owns the separate sealed `CronHookRunner` signature and `atm-runtime-test-support` owns the scripted double. BK-9 contract tests compile a BK-13-style consumer against that double and prove the full typed reply surface before BK-12/BK-13 start. BK-12 owns only `hook/reply.rs` parser implementation and authentication/template logic; it may not redefine these types or change the port signature. BK-13 uses the BK-9 types and double, without a BK-12 dependency.

## Authenticated plug-in installation and runtime provenance
The trust root is an administrator-provisioned, daemon-read-only allowlist of Ed25519 public keys, `ToolInstallerTrust { installer_id, key_id, public_key, allowed_sender, allowed_host, activated_generation, revoked_generation }`. Enrollment/revocation changes that file only through a local OS-administrator operation outside the ordinary ATM agent/roster/CLI send path; the daemon reloads it atomically and refuses malformed or world-writable trust state. If a deployment cannot protect the trust root and plugin signing key from ordinary agent processes, virtual plug-in senders remain disabled and physical-tool cutover is blocked.

The fixed local HTTP admission endpoint is `/v1/atm/tool-senders`: `POST` register/rotate and `DELETE` unregister. No remote peer may call it. The signed canonical request is `ToolInstallerRequest { schema_version, operation, installer_id, installer_key_id, sender, package_digest, registration_generation, host_id, runtime_key_id, runtime_public_key, previous_runtime_key_id, nonce, issued_at, expires_at, signature }`. Signing bytes are domain-separated (`ATM-TOOL-INSTALL-v1`) and length-prefixed in that declared field order, excluding `signature`; all text is normalized UTF-8 and timestamps are canonical UTC. BK-9 owns this DTO, OpenAPI and error-code contract. BK-14 enforces local-only endpoint admission and validates the envelope shape; BK-10 `apply_signed` verifies signature, allowlist sender/host, expiry against an injected clock, expected generation, and unique nonce, consuming the nonce and operation digest in its registry transaction. Exact replay is idempotent only for the same operation digest/result; a reused nonce with different bytes is rejected. A raw owner/sender string, ambient env and a peer-authenticated host alone never mint `TrustedInstaller`.

Registration records the runtime public key and generation. A plug-in sending via `/v1/atm/messages` supplies a signed `ToolRuntimeProof { runtime_key_id, sender, registration_generation, host_id, message_id, message_digest, nonce, signature }` using domain `ATM-TOOL-MESSAGE-v1`. BK-14 calls BK-10 `admit_signed_message`, which verifies the proof against the active registration and durably binds its nonce to the stable message id/digest before BK-14 mints an internal `ToolSenderCapability`; the proof is not itself a sender credential passed to lower crates. Remote forwarding uses a separate origin-host signed `ToolSenderAttestation` bound to the message digest, sender, registration generation, nonce and mTLS-authenticated host; the receiving daemon verifies the mTLS peer identity, the host signature, the message digest and a configured peer-host sender allowlist; it accepts only the origin daemon's attestation that the local registry admitted that exact message and rejects a bare `@tool` claim. The peer allowlist is versioned with the HTTP contract; an origin host that loses sender registration cannot produce a valid new attestation.

Rotation requires an installer-key-signed request identifying the old runtime key and a newly enrolled administrator-approved runtime key id, increments generation, and revokes old runtime proofs at the transaction boundary. Emergency revocation in the administrator trust root immediately denies new register/runtime proofs; uninstall requires the current installer key and expected registration generation. Stale/replayed, unknown/revoked key, host mismatch, sender conflict, generation conflict, malformed signature, expired request and uncertain storage commit receive distinct typed errors with recovery instructions. Rollback retains the prior key only until explicit activation; it never re-enables a revoked key silently. BK-15 owns daemon trust-root loading and end-to-end rotation/revocation/denial proof, including local/cross-host paths.

The installer trust configuration is `tool_installers.toml` in the daemon-managed configuration root, loaded only at daemon startup/atomic refresh, owner-only writable and rejected if permission checks fail. It holds *public* installer keys and their fixed allowed senders/hosts; enrollment is a local service-administrator file deployment, not an ATM roster member operation. Plug-in runtime private keys are non-exportable OS-keystore entries scoped to the installed plug-in process where supported; deployments without enforceable key isolation do not enable the virtual sender. The signed manifest/request carries `runtime_public_key` and `runtime_key_id`, which the registry records for message admission. The exact key/nonce/message-id semantics are covered by deterministic BK-10/BK-14/BK-15 fixtures.

### Acceptance criteria

- boundary:shared contracts compile in the workspace and test doubles satisfy every changed sealed port; boundary validation passes.
- boundary:ADR-061 records schema classification, version and old-consumer proofs, including the plugin registry and cron intent table; any breaking cutover has explicit approval/coexistence.
- boundary:the contract rules out `@tool` impersonation through strings/env/CLI/peer claims, defines creator-only cron explicit sender, and defines exactly one durable logical intent while marking unknown external handoff as reconciliation.
- boundary:the cron-only parser grammar rejects duplicate/unknown keys and keeps task-hook reply grammar unchanged.
- boundary:an exact installed plug-in inventory, with `randlee/atm-bd` and an explicit none or listed repo for every other sender, is attached to the BK-15 adoption bead before cutover.
- boundary:cron_contract.rs and the separate CronHookRunner scripted double compile a BK-13 consumer before BK-12 parser implementation; task HookRunner stays unchanged.
- boundary:the signed installer/runtime/peer proof DTOs, canonical bytes, pinned trust root, nonce/generation/rotation/revocation rules and local-only endpoint are versioned; raw strings and same-host trust do not grant sender authority.

## atm-bk-10: bk-10: sqlite-adapters

- **Boundary:** atm-storage-rusqlite
- **Branch:** `sprint/bk-10-sqlite-adapters`
- **Initial PR base:** `sprint/bk-9-tool-cron-contracts`

**Owned paths**

- `crates/atm-storage-rusqlite/src/cron_schedule_store.rs`
- `crates/atm-storage-rusqlite/src/tool_sender_registry.rs`
- `crates/atm-storage-rusqlite/tests/cron_schedule_store.rs`
- `crates/atm-storage-rusqlite/tests/tool_sender_registry.rs`

## Goal
Implement the two persistence adapters against the BK-9 contract.

## Deliverables
1. Persist tool registrations with authenticated owner, unique sender, idempotent reinstall and owner-only uninstall. Retain historical mail and make migration repeatable.
2. Persist cron transitions, generation/failure episode, output/lifecycle intents and delivery receipts in one conditioned SQLite transaction; implement pending/replay queries.
3. Add SQLite contract tests for stale generation, repeated failure/stop, restart, crash points, partial failures, and old-schema migration.

## This sprint does not close
Sender admission, cron execution, HTTP handoff and end-to-end delivery close in BK-12 through BK-15.

### Detailed design

The contract in `atm-bk-9` is authoritative. It defines `ToolPrincipal`, `RegisteredToolSender`, `TrustedInstaller`, `ToolSenderRegistry`, `CronFailureKind`, `CronFailureKey`, `CronRunTransition`, `CronNoticeKey`, `CronLifecycleEvent`, `CronDeliveryIntent`, `CronTransitionWrite`, `CronTransitionStore`, `CronTickReply`, `CronStopReply`, `CronModuleOutput`, `CronRunPermit`, `CronLifecycleNudger` and typed receipts. It fixes the cron-only JSON grammar, sender authority, generation fence, SQL intent schema, lifecycle template slots, hook-schema version, compatibility and recovery semantics. See its complete design, not an assignment summary. This sprint owns only the boundary named here; runtime reach across crates closes in `atm-bk-15`.
The `CronTransitionStore::commit` transaction conditions on `(id,generation,active)`, writes result and all intents atomically, and never mutates a restarted row. Error suppression compares typed stable `CronFailureKey`, not display text. `StopCheckContinues` may persist output intents without touching last outcome. Manual end writes one stopped intent per row in its deactivate transaction. `pending` is ordered by transition/output index, and `mark_delivered` is keyed and idempotent. A shared schema migration retains physical-tool history without granting transient virtual authority. Tests use a temporary SQLite file, explicit failpoints and reopen, not concurrent sleeps.

The BK-9 `ToolSenderRegistry::apply_signed` contract owns signature, trust-root, nonce and generation verification inside the SQLite registration transaction. `admit_signed_message` durably binds `(runtime_key_id,nonce)` to `(message_id,digest,sender,generation)` and returns the same receipt on exact replay, so a canonical mail write uncertain after admission can be retried with the identical id/body. A reused nonce with altered bytes fails. Tests inject failure before and after nonce commit and reopen the store.

### Acceptance criteria

- boundary:atm-storage-rusqlite contract tests pass for registry and cron transition adapters, including crash/reopen, prior schema, stale generation and old binary fixture.
- boundary:commit failure leaves neither result update nor intent; repeated logical transitions never duplicate intents.
- boundary:plugin uninstall affects only owned registration, not other senders or historical mail.

## atm-bk-11: bk-11: cron-docs

- **Boundary:** atm CLI help and installed user docs
- **Branch:** `sprint/bk-11-cron-docs`
- **Initial PR base:** `sprint/bk-9-tool-cron-contracts`

**Owned paths**

- `crates/atm/src/commands/cron.rs`
- `crates/atm/src/commands/help.rs`
- `crates/atm/tests/cli_surface.rs`
- `crates/atm/examples/gen_cli_docs.rs`
- `docs/user-documents/cron.md`
- `docs/user-documents/cli-reference.md`
- `docs/user-documents/README.md`
- `docs/user-documents/nudge-templates.md`
- `docs/user-documents/hooks.md`
- `docs/user-documents/examples/cron`
- `crates/atm/tests/cron_examples.rs`
- `scripts/verify_release_archive.py`
- `release/publish-artifacts.toml`

## Goal
Make the frozen cron contract discoverable and its examples executable from docs and CLI help.

## Deliverables
1. Complete Clap help for `atm cron start|end|list` and `atm help cron`, then regenerate reference.
2. Add installed `cron.md` with all documented sender, output, lifecycle, stop, error, template and recovery cases; update existing hooks and nudge guides.
3. Own one fixture manifest under examples/cron and a deterministic verifier for the stable Clap and cron-expression parsers, frozen BK-9 JSON grammar and contract doubles.
4. Prove staged archive membership and installed-help link resolution on all release target formats.

## This sprint does not close
New cron-only reply parser, `AtmCron` runner, live daemon delivery and cross-crate example execution; BK-15 consumes these fixture IDs for that integration proof.

### Detailed design

## Documentation boundary
BK-11 changes the CLI presentation and installed user-doc surface only. BK-9 owns contracts; BK-10 SQLite, BK-12 core parser/sender/template, BK-13 cron execution, BK-14 HTTP delivery, and BK-15 composed runtime behavior. This sprint is governed by `REQ-P-USER-DOCS-001/002` and `ADR-025`. The docs source remains `docs/user-documents/`; the installer/publisher copies that tree to `<install-root>/share/doc/atm/` relative to the resolved binary. No `ATM_HOME` lookup and no second document root.

## Explicit CLI/help model
The existing Clap definitions are the single command schema:

```rust
#[derive(Parser)]
struct CronCommand { command: CronSubcommand }
#[derive(Subcommand)]
enum CronSubcommand { Start(CronStartArgs), End(CronEndArgs), List(CronListArgs) }
```

Expand `about`, `long_about`, per-field `help`/`long_help`, and generated reference for every item. `start`: name, six/seven-field UTC expression (seconds first, optional year and supported `@` aliases), module path, phase, exactly one `--until` RFC3339 instant or `--until-hook`, repeated `--arg`, `--cwd`, `--custom` input JSON object, module TIMEOUT_MS/CRON_TIMEOUT_MS cap, fixed persisted jitter. `end`: phase and optional name, with clear manual-stop semantics. `list`: optional phase, active and ended rows, `--json`, created_by, generation/jitter and `last_run_at`/`last_outcome`. Clarify that `cron@tool` is the omitted sender for action JSON and an explicit `from` equal to the persisted creator may override it under BK-9 authorization; it is not a sender switch on `atm cron start`.

Add `HelpTopic::Cron` to the closed `HelpTopic` enum and `ALL` list in `crates/atm/src/commands/help.rs`, with name/title/summary/body, a `doc_link_for_topic` mapping to `cron.md`, and JSON `installed_doc_topic_path`. Keep concise help and long-form guide separate. `docs/user-documents/cron.md` gets the accepted metadata header with `reviewed_for_release`, relative links to hooks, nudge templates, errors and troubleshooting, and no direct SQL/editing steps. Link it from `README.md`. The packaging contract copies the whole corpus; update the canonical verifier's required-file set only if it uses an explicit list, then prove the archive/installed tree contains `share/doc/atm/cron.md` on each supported platform.

## Guide's normative examples
Include exact JSON and rendered-output examples for (a) pass-through plus custom message, (b) handled action plus explicit ordinary sender, (c) custom error mail that does not itself fail the schedule, (d) stop-hook true with a custom final message followed by stopped nudge, (e) timeout/error lifecycle nudge and repeat suppression. The guide must show these two separate surfaces side by side:

```text
<atm cron name="nightly" event="error" error="timeout: ATM_HOOK_TIMEOUT">
<atm cron name="nightly" event="stopped" message="phase complete">
```

These are daemon lifecycle nudges, not unread messages. Custom output is normal unread mail from `cron@tool` to `created_by@team`, with category `cron` and `output:message|error` tag, visible through normal `atm list/read/search`; it has no implicit ack or task. Describe `atm teams ...` nudge-template commands for kind `cron`, the allowed `{{name}}`, `{{event}}`, `{{detail_attribute}}` placeholders and disable effect. Use the frozen BK-9 command grammar and existing Clap names/flags, then have BK-15 verify them against the composed command path.

## Mechanical checks
Add a deterministic table of expected Clap paths and option IDs and assert each is present with non-placeholder help. Compare the generated `cli-reference.md` to the live Clap tree. Validate the guide's fenced JSON/XML/shell examples with a new deterministic documentation fixture verifier (there is currently no canonical cron example verifier), including the existing Clap and cron-expression parsers for `atm cron start|end|list` command examples. BK-15 later verifies replies with the new production cron-only parser and executes representative fixtures through `AtmCron`. Build a staged install tree from fixture paths and assert `atm help cron --json` resolves `installed_doc_topic_path` to an existing `cron.md` next to the resolved executable, including symlinked executable; verify release archive manifest includes it. No daemon startup, wall-clock timing, or network needed.

## Example execution proof
Put every shell command and cron reply JSON example in a machine-readable fixture manifest consumed by the documentation gate. For each command, use the real Clap parser and a fixture module/path to prove flags, `--until` versus `--until-hook`, `--arg --json` hyphen values, RFC3339 normalization, six/seven-field expressions, and supported `@` aliases. For each reply, verify JSON syntax and the frozen BK-9 contract shape with a contract double. BK-15 feeds the same bytes through the new cron-only parser and the controlled `AtmCron` runner/store to prove sender/recipient, lifecycle versus mailbox record, dedup and `last_outcome`. Do not merely grep examples. Archive checks must inspect actual staged manifests for macOS, Linux, and Windows and resolve `atm help cron --json` against the installed relative path.

## Deterministic coverage review and implementation gate
Existing `atm help` tests prove topic-link resolution and the CLI-surface baseline, but there is no cron topic and no execution proof for the current hooks.md cron examples. Make `docs/user-documents/examples/cron/` the single fixture source for every command, JSON reply, and expected mail/nudge shown in `cron.md` and generated reference. A test extracts or maps each documented block by stable fixture ID and fails if text drifts. Parse shell arguments with the real Clap command tree, validate expressions through the production cron parser, validate replies against BK-9 grammar fixtures, and reserve production cron-only parser and `AtmCron` manual-clock execution for BK-15. BK-15 asserts the exact documented output, sender, recipient, classification, stop/error suppression, and rejected counterexamples; avoid merely checking that JSON parses. Include six and seven fields, supported aliases, five-field refusal, relative `--module`/`--cwd`, repeated `--arg` including `--json`, RFC3339 UTC normalization, mutually exclusive stop flags, invalid `--custom`, and explicit creator `from` versus impersonation refusal. The current hooks.md examples must either pass the same fixtures or be updated with a cross-link; no stale contradictory examples. Compare Clap-generated reference/help text with checked-in docs. Build representative staged macOS/Linux tar and Windows zip archives from the real release manifest and assert `share/doc/atm/cron.md` is present; resolve `atm help cron --json` from a staged binary and symlink. No wall-clock, daemon, network, or load-dependent assertions.

The fixture source is owned only here. BK-15 imports fixture IDs, never copies JSON or normative expected output. The verifier must fail on a guide/example mismatch. Use the real `release/publish-artifacts.toml` bundled-path declaration and `verify_release_archive.py`; there is no pre-existing canonical cron example verifier.

This docs sprint may close once the frozen fixtures, Clap/expression examples, rendered guide and staged archive checks pass. It does not assert new-parser or daemon behavior. BK-15 acceptance runs the same fixture IDs against BK-12/BK-13/BK-14 production implementations before the feature can close.

### Acceptance criteria

- boundary:docs all command flags and guide examples match the frozen BK-9 contract and the live Clap parser and frozen BK-9 reply grammar.
- boundary:docs fixture verifier rejects any mismatched example, including five-field expression and stop flag conflict; invalid sender and malformed-output examples are marked as rejection fixtures for BK-15 production execution.
- boundary:docs staged macOS/Linux tar and Windows zip contain cron.md and installed `atm help cron --json` resolves it.

## atm-bk-12: bk-12: core-auth-hooks

- **Boundary:** atm-core
- **Branch:** `sprint/bk-12-core-auth-hooks`
- **Initial PR base:** `sprint/bk-9-tool-cron-contracts`

**Owned paths**

- `crates/atm-core/src/caller_context.rs`
- `crates/atm-core/src/team_admin`
- `crates/atm-core/src/send`
- `crates/atm-core/src/hook/reply.rs`
- `crates/atm-core/tests/tool_sender.rs`
- `crates/atm-core/tests/cron_reply.rs`
- `crates/atm-core/tests/cron_nudge.rs`

## Goal
Implement core sender authorization, cron reply parsing and lifecycle nudge rendering.

## Deliverables
1. Resolve `cron@tool` only from an internal schedule capability and fixed plugin senders only from registered installer capability; reject CLI/env/JSON/peer spoofing.
2. Preserve authenticated ordinary `--from`/`--as` precedence and creator-only cron override.
3. Implement the cron-only streaming parser in `hook/reply.rs` for BK-9-owned `hook/cron_contract.rs` types, exact schemas and typed output validation without widening task-hook replies.
4. Render `cron` template override with trusted message/error attribute and escaping, alongside unchanged eleven post-send templates. Add unit/contract tests.

## This sprint does not close
SQLite transactions, cron scheduling, HTTP handoff and composed behavior close elsewhere.

### Detailed design

The contract in `atm-bk-9` is authoritative. It defines `ToolPrincipal`, `RegisteredToolSender`, `TrustedInstaller`, `ToolSenderRegistry`, `CronFailureKind`, `CronFailureKey`, `CronRunTransition`, `CronNoticeKey`, `CronLifecycleEvent`, `CronDeliveryIntent`, `CronTransitionWrite`, `CronTransitionStore`, `CronTickReply`, `CronStopReply`, `CronModuleOutput`, `CronRunPermit`, `CronLifecycleNudger` and typed receipts. It fixes the cron-only JSON grammar, sender authority, generation fence, SQL intent schema, lifecycle template slots, hook-schema version, compatibility and recovery semantics. See its complete design, not an assignment summary. This sprint owns only the boundary named here; runtime reach across crates closes in `atm-bk-15`.
Core must never accept a public constructor for `ToolSenderCapability`, `TrustedInstaller` or `CronRunPermit`. `CronTickReply` and `CronStopReply` are selected only for cron hook names; duplicate keys fail at every nesting level. `CronOutputBody` preserves valid module text verbatim while `CronDiagnosticDetail` is redacted. The cron nudge renderer uses its own `name`, `event`, `detail_attribute` map, validates overrides at set time, and escapes attribute values. Tests use explicit caller contexts, parser bytes and hostile strings.

The BK-9 `cron_contract.rs` reply types and separate `CronHookRunner` signature are frozen inputs. This sprint implements parsing and sender/template logic only; its tests consume the BK-9 scripted runner double and do not edit contract modules.

### Acceptance criteria

- boundary:atm-core tests prove every allowed/denied sender matrix cell, duplicate/unknown JSON key, content bound and nudge escape/disable case.
- boundary:ordinary task-hook and send behavior remains byte/semantically compatible where governed; no generic `@tool` selector exists.

## atm-bk-13: bk-13: cron-engine

- **Boundary:** atm-cron
- **Branch:** `sprint/bk-13-cron-engine`
- **Initial PR base:** `sprint/bk-9-tool-cron-contracts`

**Owned paths**

- `crates/atm-cron/src`
- `crates/atm-cron/tests`

## Goal
Implement cron execution and typed transitions against BK-9 test doubles.

## Deliverables
1. Execute cron.tick/cron.stop through the cron-only runner and classify success/failure/stopped/skipped state.
2. Commit `StopCheckContinues`, success, failure and stopped transitions with generation fencing; dispatch actions with a permit checked at canonical admission.
3. Keep custom output ordered and separate from lifecycle errors; preserve last_outcome projection.
4. Add controlled-clock worker tests for repeat/changed failure, recovery, stop variants, restart, stale generation and partial action commit.

## This sprint does not close
Concrete SQLite persistence or daemon nudge/mail transport; BK-10/BK-14/BK-15 own those.

### Detailed design

The contract in `atm-bk-9` is authoritative. It defines `ToolPrincipal`, `RegisteredToolSender`, `TrustedInstaller`, `ToolSenderRegistry`, `CronFailureKind`, `CronFailureKey`, `CronRunTransition`, `CronNoticeKey`, `CronLifecycleEvent`, `CronDeliveryIntent`, `CronTransitionWrite`, `CronTransitionStore`, `CronTickReply`, `CronStopReply`, `CronModuleOutput`, `CronRunPermit`, `CronLifecycleNudger` and typed receipts. It fixes the cron-only JSON grammar, sender authority, generation fence, SQL intent schema, lifecycle template slots, hook-schema version, compatibility and recovery semantics. See its complete design, not an assignment summary. This sprint owns only the boundary named here; runtime reach across crates closes in `atm-bk-15`.
The worker uses `ManualClock`, scripted `HookRunner`, `CronTransitionStore` double and controlled action barrier. `cron.stop(false)` outputs commit before tick; `cron.stop(true)` outputs and stopped intent share one transition; until and manual end have the same one-time stopped semantics. `SkippedInFlight` does not reset suppression. Parsing and validating entire replies precedes effects. A failed later action records partial commit but never pretends rollback.

The BK-9 `hook/cron_contract.rs` exports `CronTickReply`, `CronStopReply`, `CronHookOutcome`; `CronHookRunner` and its scripted double are also BK-9 artifacts. Compile and test this boundary against them without waiting on the BK-12 parser implementation.

### Acceptance criteria

- boundary:atm-cron tests deterministically prove transition order and result classes with no sleep, event-loop or load-sensitive assertion.
- boundary:a generation change blocks each new action and transition write without erasing an action already committed.

## atm-bk-14: bk-14: http-delivery

- **Boundary:** atm-http-runtime
- **Branch:** `sprint/bk-14-http-delivery`
- **Initial PR base:** `sprint/bk-9-tool-cron-contracts`

**Owned paths**

- `crates/atm-http-runtime/src`
- `crates/atm-http-runtime/tests`

## Goal
Implement HTTP ingress, cron intent handoff and ordinary-mail admission against BK-9 ports.

## Deliverables
1. Implement `/v1/atm/tool-senders` local-only signed register/rotate/unregister admission and signed runtime/peer sender provenance on message ingress; reject a bare peer-asserted `@tool` address.
2. Deliver cron lifecycle nudges through idempotent key-aware transport without mailbox/task/ack rows; deliver custom output via canonical mail with stable message id.
3. Persist accepted/failed/unknown handoff receipts through the transition port and expose uncertain recovery.
4. Add HTTP boundary tests for spoofing, receipt replay, disabled template, missing creator and cross-host dedup.

## This sprint does not close
Daemon composition and installed CLI flow; BK-15 owns those.

### Detailed design

The contract in `atm-bk-9` is authoritative. It defines `ToolPrincipal`, `RegisteredToolSender`, `TrustedInstaller`, `ToolSenderRegistry`, `CronFailureKind`, `CronFailureKey`, `CronRunTransition`, `CronNoticeKey`, `CronLifecycleEvent`, `CronDeliveryIntent`, `CronTransitionWrite`, `CronTransitionStore`, `CronTickReply`, `CronStopReply`, `CronModuleOutput`, `CronRunPermit`, `CronLifecycleNudger` and typed receipts. It fixes the cron-only JSON grammar, sender authority, generation fence, SQL intent schema, lifecycle template slots, hook-schema version, compatibility and recovery semantics. See its complete design, not an assignment summary. This sprint owns only the boundary named here; runtime reach across crates closes in `atm-bk-15`.
The HTTP adapter must verify a host-bound sender registration/capability, not trust a string from an authenticated host. `CronLifecycleNudger::deliver` uses `CronNoticeKey`; a receiver without durable dedup reports unknown and does not claim exactly once. Module mail uses a preallocated id and canonical writer under BK-9 capability. Failure cases are explicit typed receipts; the queue does not blindly retry unknown acceptance.

Consume BK-9 `ToolInstallerRequest`, `ToolRuntimeProof`, `ToolSenderAttestation` and `ToolInstallerTrust` contracts. Call BK-10 signed registry admission, which verifies Ed25519 signatures over domain-separated canonical bytes, injected-clock expiry, sender/host allowlist, generation and nonce replay before BK-14 mints any internal capability. BK-14 separately checks local-only route access and peer-host attestation/allowlist. Reject remote calls to the installer endpoint. Use BK-10 `apply_signed` and `admit_signed_message` so signatures/nonces are checked in the registry transaction; test rotation, revocation, stale/reused nonce, raw claimed sender and authenticated-peer-without-tool-proof.

After a successful runtime admission, write the mail with the proof-bound stable `message_id`. On uncertain write, retry the identical proof, id and body; the registry returns the prior admission receipt and the canonical writer deduplicates the id. Never generate a fresh id for a retry.

### Acceptance criteria

- boundary:atm-http-runtime tests prove spoof refusal, one logical lifecycle handoff, stable-id module mail replay and no lifecycle unread/task/ack row.
- boundary:accepted-before-mark and unknown receipts remain inspectable and safe under restart without false exactly-once claims.
- boundary:signed local installer admission and signed runtime/peer proofs are verified before capability minting; invalid, replayed, rotated, revoked or foreign-host proofs fail with typed codes.

## atm-bk-15: bk-15: daemon-integration

- **Boundary:** atm-daemon-bootstrap composition
- **Branch:** `sprint/bk-15-daemon-integration`
- **Initial PR base:** `sprint/bk-14-http-delivery`

**Owned paths**

- `crates/atm-daemon-bootstrap/src`
- `crates/atm-daemon-bootstrap/tests`
- `crates/atm/src/commands/send.rs`
- `crates/atm/src/commands/ack.rs`
- `crates/atm/src/commands/teams.rs`
- `crates/atm/src/commands/sender_roster.rs`
- `crates/atm/tests/cron_integration.rs`
- `crates/atm/tests/tool_sender_integration.rs`

## Goal
Close the tool sender and cron lifecycle features through production daemon composition.

## Deliverables
1. Wire SQLite, core, cron and HTTP ports plus protected installer trust-root loading in Tokio/Axum daemon bootstrap; no legacy daemon path.
2. Update `atm send --from` and team template/roster CLI without granting `@tool` impersonation; verify the existing `atm cron` command routing without editing BK-11-owned `cron.rs`.
3. Consume BK-11 fixture IDs in end-to-end tests for every documented success/refusal, sender/recipient, custom mail, lifecycle nudge and failure/recovery case.
4. Gate physical-tool cutover on the tracked atm-bd installer adoption receipt and previous-consumer/migration proof; run release validation and quality-mgr QA.

## This sprint does not close
No further runtime boundary remains after acceptance; external plugin code is the separately tracked prerequisite.

### Detailed design

The contract in `atm-bk-9` is authoritative. It defines `ToolPrincipal`, `RegisteredToolSender`, `TrustedInstaller`, `ToolSenderRegistry`, `CronFailureKind`, `CronFailureKey`, `CronRunTransition`, `CronNoticeKey`, `CronLifecycleEvent`, `CronDeliveryIntent`, `CronTransitionWrite`, `CronTransitionStore`, `CronTickReply`, `CronStopReply`, `CronModuleOutput`, `CronRunPermit`, `CronLifecycleNudger` and typed receipts. It fixes the cron-only JSON grammar, sender authority, generation fence, SQL intent schema, lifecycle template slots, hook-schema version, compatibility and recovery semantics. See its complete design, not an assignment summary. This sprint owns only the boundary named here; runtime reach across crates closes in `atm-bk-15`.
Only composition and sender/team CLI wiring belong here; BK-11 owns cron.rs help text while this sprint tests its existing routing. Use a controlled in-process daemon, test store and fake handoff, not a live host or sleeps. The integration suite consumes BK-11 fixtures and asserts concrete local/HTTP behavior and absence of task/mail rows for lifecycle nudges. Verify old physical-tool state migrates without message loss, and old plugin cannot be cut off before its adopter is deployed.

Load the administrator-provisioned trust root with permission checks and atomic refresh; keep tool sender admission disabled if key protection cannot be enforced. Compose BK-10 nonce/registry transactions and BK-14 signed local installer/runtime/peer ingress. Controlled integration fixtures cover valid enrollment, key rotation, emergency revocation, exact retry, conflicting replay, host mismatch, uninstall, restart and rollback; BK-11 examples then run through the same production route.

### Acceptance criteria

- req:REQ-P-TOOL-IDENTITY-001 `cron@tool` and registered fixed plugin senders work through real composition; arbitrary user, env, CLI and cross-host claims fail.
- req:REQ-P-CRON-005 first/repeated/changed errors, success reset, all normal stops, custom normal mail and nudge overrides match docs through real daemon adapters.
- req:REQ-P-USER-DOCS-002 every BK-11 fixture executes through the production command and daemon path with asserted output, including denied cases.
- cutover is blocked until the atm-bd installer-adoption bead proves registration/uninstall/migration compatibility and governing schema approvals exist.
- req:REQ-P-CRON-001 start/end/list and six/seven-field, stop-condition, creator and persisted-state cases execute through the real CLI/daemon path.
- req:REQ-P-CRON-002 the controlled scheduler ticks, stops and resumes without replay or extra side effects.
- req:REQ-P-CRON-003 the persistent failure episode creates one lifecycle nudge per stable cause and resets after success, including restart, disabled template and unknown handoff cases.
- req:REQ-P-CRON-004 custom message/error outputs become canonical unread mail with exact sender, recipient, classification and stable id; malformed replies and action partial commits have the documented errors.
- req:REQ-P-SEND-001 full-address `--from` and legacy `--as` follow authenticated caller context, while cron explicit sender is restricted to its persisted creator.
- req:REQ-P-TOOL-IDENTITY-001 external plugin registration and runtime send require BK-9 signed proofs, local endpoint admission, nonce/generation fencing, protected trust root and rotation/revocation; a raw sender or authenticated host alone is refused.
