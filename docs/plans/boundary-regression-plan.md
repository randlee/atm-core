# Boundary Regression Fix Plan — the 22 sc-boundary Findings

status: revised_after_review_round_1
branch: plan/boundary-regression (off develop @ c94d544fe)
tracking: GitHub issue #1028; triage waiver AO2-SCBOUNDARY-DEBT-001 (phase-ao2)
author: fenix (team-lead), from 4 independent code/lint analyses, 2026-08-26

## Context

Fixing QA-RUSTQA-AO2-001 (`.just/run_lint.py` silently excluded `sc-portability`/`sc-boundary`
from `just lint all`/`validate`; fixed at `9b7f022e4` on integrate/phase-ao2) armed the
sc-boundary lint for the first time in full validation. It reports 22 findings, verified
identical on `develop` (pre-existing; zero phase-ao2 delta). They were waived for the AO2
phase gate (AO2-SCBOUNDARY-DEBT-001) and this plan is the follow-up.

**Goal: clear all 22 findings without loosening any boundary.** After analysis, the 22 decompose as:

| Disposition | Count | Sections |
|---|---|---|
| Code fix, no boundary change | 10 | §1 (easy), §2 (harder) |
| Lint defect / miscalibration in our own `sc-lint-boundary` crate | 11 | §4 |
| True architectural cycle requiring design revisit | 1 | §3 |

No finding is resolved by weakening a rule threshold, deleting a rule from `just validate`,
or blanket-baselining. The only suppression used anywhere is the lint's own purpose-built
`#[sc_lint(boundary.allow("cycle.recursive_value_container"))]` opt-in on one genuine
recursive value container (§1.6) — the exact pattern that marker was designed for.

### Lint semantics (basis for classification)

`sc-lint-boundary` (in-workspace crate) builds an owner graph from `references_type`/
`references_expr` edges and reports three finding kinds (`crates/sc-lint-boundary/src/analysis.rs`):

- **SCB-CYCLE-001 "architectural cycle across owners"** — a >1-owner SCC. Every `type`/`trait`/
  `module` node is its own owner (`analysis.rs:472-494`); same-module types are deliberately
  NOT collapsed (pinned by `tests.rs:1223-1260`).
- **SCB-CYCLE-002 "type/method self-loop"** — an *inherent* method whose body has an
  expression-position reference to its own type while its signature has none
  (`is_type_method_self_loop`, `analysis.rs:149-166`). This is a heuristic: it cannot
  distinguish `Self::private_helper(...)` delegation from architectural recursion.
- **SCB-CYCLE-003 "trait-impl self-loop via Trait"** — any method of a non-ignored trait impl
  referencing its own type, signature or body (`classify_trait_impl_self_loop`,
  `analysis.rs:179-198`). The static ignore list (`config/defaults.toml`) covers only
  std/serde derive-style traits, so every port-trait implementation
  (`impl MessageStore for SqliteMessageStore`, …) fires.

Suppression mechanisms that exist today: `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]`
(multi-owner cycles; all owners must opt in) and `#[sc_lint(boundary.allow("cycle.type_method_self_loop"))]`
(gates both 002 and 003). There is **no baseline/ignore-file mechanism** — which is fine, we do not want one.

---

## 1. Easy fixes (mechanical, exact change specified, no behavior change)

Each item lists the exact change. All are independent; land as one commit series on a single
fix worktree. Estimated total: ~half a day including test runs.

### 1.1 `SearchInput` ↔ `SearchRequest` cycle [SCB-CYCLE-001]
- Cycle: `crates/atm-core/src/search.rs:29` (`SearchRequest.query: SearchInput`, load-bearing)
  vs `search.rs:265-270` (`SearchInput::into_request`, incidental — conversion placed on the
  wrong type).
- **Change**: delete `SearchInput::into_request`; add
  `impl From<SearchInput> for SearchRequest { fn from(query: SearchInput) -> Self { Self { query, lifecycle: None } } }`.
  Update 2 call sites: `crates/atm-core/src/search.rs:894` and
  `crates/atm/src/commands/search.rs:185` (`x.into_request()` → `SearchRequest::from(x)`).
- *(review r1)* `into_request` is on a `pub` type in a library crate: run a public-API audit
  (grep downstream crates + `cargo doc` surface) for external callers before deleting; if any
  exist outside the workspace surface, deprecate-and-delegate for one release instead.

### 1.2 Teams command family 9-type cycle [SCB-CYCLE-001]
- Cycle: `TeamsSubcommand` wraps 7 command structs (`crates/atm/src/commands/teams.rs:38-45`),
  and each struct's `run()` calls `TeamsCommand::reload_runtime_view()`
  (`teams.rs:215,266,291,311,331,359,382`). The helper (`teams.rs:202-204`) takes no `self`
  and nothing `TeamsCommand`-specific; `BackupCommand` never calls it and is absent from the
  cycle — proof the coupling is organizational.
- **Change**: hoist `reload_runtime_view` out of `impl TeamsCommand` into a private
  module-level `async fn reload_runtime_view() -> Result<()>`; change the 7 call sites to the
  bare call. Zero API change.

### 1.3 `SharedLogBuffer` ↔ `SharedLogWriter` test-only cycle [SCB-CYCLE-001]
- Cycle: `crates/atm/src/composition.rs:542` (`struct SharedLogWriter(SharedLogBuffer)`) vs
  `composition.rs:544-549` (`MakeWriter for SharedLogBuffer` returns `SharedLogWriter`).
- **Change**: `SharedLogWriter` holds the raw handle it actually uses:
  `struct SharedLogWriter(Arc<Mutex<Vec<u8>>>);` and
  `fn make_writer(&'a self) -> Self::Writer { SharedLogWriter(self.0.clone()) }`; adjust the
  `Write` impl body accordingly. Test-only code.

### 1.4 `MessageEnvelope` ↔ `RawMessageEnvelope` cycle — dead code [SCB-CYCLE-001]
- Cycle: `crates/atm-storage/src/schema/inbox_message.rs:244-267`
  (`impl From<MessageEnvelope> for RawMessageEnvelope`) has **zero callers repo-wide**;
  `MessageEnvelope` derives `Serialize` directly (`:137`). Only the reverse direction
  (`:269-295`, deserialize shadow-struct) is load-bearing.
- **Change**: delete the unused `impl From<MessageEnvelope> for RawMessageEnvelope` block
  (`inbox_message.rs:244-267`). Verify with `cargo build && cargo clippy`.

### 1.5 `BoundedHostNudgeInjector` trait-impl self-loop [SCB-CYCLE-003]
- Cause: `crates/atm-graft/src/runtime.rs:217-221` — the `HostNudgeInjector` trait method is
  literally `Self::inject_nudge(self, nudge)` forwarding to an inherent method (`:223-242`)
  that has **no other caller** (verified). Genuinely redundant wrapper.
- **Change**: inline the inherent `inject_nudge` body into the trait method; delete the
  inherent method. Field accesses (`self.helper_budget`, `self.injector`) do not create
  self-loop edges.
- *(review r1)* add/keep tests that invoke the behavior **through the trait path**
  (`<BoundedHostNudgeInjector as HostNudgeInjector>::inject_nudge` / dyn dispatch), since the
  inherent path the existing tests may use is being deleted.

### 1.6 `LogFieldMap` ↔ `LogFieldValue` — genuine recursive value container [SCB-CYCLE-001]
- Cause: `crates/atm-core/src/observability.rs:343` (`LogFieldValue::Object(LogFieldMap)`) and
  `:437` (`LogFieldMap.entries: Vec<(LogFieldKey, LogFieldValue)>`) — a textbook recursive
  JSON-value shape. This is exactly the case the lint's
  `component_allows_recursive_value_container` opt-in (`analysis.rs:200-213`) was built for;
  the marker is simply absent (it has never been used outside the lint's own tests).
- **Change**: add `sc-lint-attributes` as a dependency of `atm-core`;
  `use sc_lint_attributes::sc_lint;`; annotate **both** declarations
  (`observability.rs:337` and `:436`) with
  `#[sc_lint(boundary.allow("cycle.recursive_value_container"))]`. Structural change: none.
  This is the designed, per-type, all-owners-must-opt-in mechanism — not a loosening.

**§1 clears 6 findings.**

---

## 2. More difficult changes (real refactors; no API break, no boundary change)

### 2.1 `HelperThreadBudget` ↔ `HelperThreadPermit` — RAII permit decoupling [SCB-CYCLE-001]
- Cause: standard semaphore/permit idiom — permit holds `Arc<HelperThreadBudget>` so `Drop`
  can release (`crates/atm-graft/src/runtime.rs:125-155`).
- **Change**: share only the counter the permit actually needs:
  ```rust
  struct HelperThreadBudget { max_inflight: usize, inflight: Arc<AtomicUsize> }
  fn try_acquire(&self) -> Option<HelperThreadPermit> {
      … Some(HelperThreadPermit { inflight: Arc::clone(&self.inflight) })
  }
  struct HelperThreadPermit { inflight: Arc<AtomicUsize> }
  impl Drop for HelperThreadPermit {
      fn drop(&mut self) { self.inflight.fetch_sub(1, Ordering::SeqCst); }
  }
  ```
  Call sites (`runtime.rs:245,289,323`) unaffected (`Arc<T>` deref). Behavior identical, but
  this is concurrency-sensitive code: requires a careful review pass plus the existing
  helper-budget tests. **MEDIUM.**
- *(review r1)* correctness condition: the permit must clone **the same counter instance the
  successful CAS/increment was performed on** — clone `Arc` inside the success branch of
  `try_acquire`, never construct a fresh counter. Required tests: failed-acquire leaves
  `inflight` unchanged; N concurrent acquires never exceed `max_inflight`; permit drop
  releases exactly one slot; dropping the `HelperThreadBudget` while permits are live must
  not invalidate outstanding permits (counter `Arc` keeps it alive).

### 2.2 `ScComposeTemplateComposer` — hoist stateless helpers to free functions
    [clears both SCB-CYCLE-002 #10 and SCB-CYCLE-003 T5]
- Cause: 11 stateless translation/validation helpers grouped as associated fns in the inherent
  impl (`crates/atm-template-sc-compose/src/lib.rs:40-229`); both the inherent cluster and the
  `TemplateComposer` trait methods (`:233-…`, e.g. `:243,262,269,271,289`) call them via
  `Self::`, producing one 002 and one 003 finding.
- **Change**: move the helpers (`inspect_raw_file`, `verify_unchanged`, `checked_body`,
  `to_sc_output_format`, `from_sc_output_format`, `inspection_error`, `inspection_parse_error`,
  `composition_error`, `template_load_error`, `upstream_frontmatter`, `upstream_references`)
  to private module-level free functions — none takes `&self`, so this is a mechanical
  `Self::x(...)` → `x(...)` sweep within one file. Visibility unchanged (all private to the
  adapter module). **MEDIUM (size), LOW (risk).** Clears 2 findings with zero suppressions.
- *(review r1)* the helper list above was incomplete: `source_text` (lib.rs:183) and
  `hash_api_error` (lib.rs:345) are also `Self::` associated helpers. Implementation step 1
  is an exhaustive `Self::` sweep of the file — hoist **every** associated helper (or record
  why an omission cannot contribute a self-loop edge) before declaring the findings cleared.

### 2.3 `SendCommand` — hoist error-builder helpers to free functions [SCB-CYCLE-002 #5]
- Cause: `message_validation_error`/`template_load_error` are pure `AtmError` builders called
  via `Self::` from `build_message_source`/`build_template_source`/`read_var_file`
  (`crates/atm/src/commands/send.rs:274-407`, ~12 call sites).
- **Change**: same hoist-to-free-function pattern as 2.2, within `send.rs`. **MEDIUM (small).**
- Note: if §4.2 (the lint heuristic fix) lands first, this item becomes unnecessary; do 2.2/2.3
  only if we want the code cleared independent of lint calibration. Recommended: do 2.2 (it
  clears a 003 finding the heuristic fix alone might not), skip 2.3 in favor of §4.2.

**§2 clears 3–4 findings (2.1, 2.2×2, optionally 2.3).**

---

## 3. True boundary violations requiring revisiting (design discussion before dispatch)

### 3.1 `atm-core`: module `ack` ↔ module `send` [SCB-CYCLE-001] — the one real cycle
- Evidence of genuine bidirectional design:
  - ack → send: `src/ack/mod.rs:10` imports `SendMessageSource/SendOutcome/SendRequest/WriteOutcome`;
    `ack/mod.rs:194` calls `crate::send::write_mail_with_runtime(...)`.
  - send → ack: `src/send/mod.rs:8` imports `AckOutcome`
    (`WriteOutcome::Acknowledged(AckOutcome)`, `send/mod.rs:265-267`);
    `send/mod.rs:296` `PreparedWrite.acknowledgement: Option<crate::ack::ResolvedAcknowledgement>`;
    `send/mod.rs:602` takes `crate::ack::AtomicAcknowledgementWrite`;
    `send/mod.rs:538,579` call `crate::ack::admit_acknowledgement_write[_async]`.
- `send` owns the single canonical write pipeline that intentionally branches into
  Sent/Acknowledged outcomes; it structurally needs ack's outcome/request shadow types. This
  is the intended "one write, two outcome shapes" design — a real module cycle, not lint noise.
- **Proposed direction** (revised per review r1; needs sign-off before dispatch): introduce a
  narrow sibling module (e.g. `atm_core::write`) owning the generic canonical-write /
  ack-admission contract — the shared shapes (`AckOutcome`, `AckReplyDisposition`,
  `ResolvedAcknowledgement`, `AtomicAcknowledgementWrite`, `ReplyTarget`, plus the admission
  entry points both sides call) — with **both** `ack` and `send` depending on it one-way.
  `ack` keeps `pub use` re-exports so its public path surface is unchanged.
- Rationale for rejecting the draft's original "move ack types into send" option: the calls
  are genuinely bidirectional — `ack/mod.rs:194` calls send's writer while
  `send/mod.rs:538,579` call ack's admission — so relocating types into `send` would produce
  a *misleading* one-way `ack → send` picture while the admission control flow still runs the
  other way. A third owner models the real contract honestly.
- Remaining open questions: exact member set of the new module (types only vs types +
  admission fns); serde/public-API compatibility of moved types (re-exports should preserve
  paths — verify against atm-graft/daemon consumers). Full design review required.
- **HARD.** Do not dispatch until the direction is agreed (per standing rule: architectural
  findings get discussed before any fix dispatch).

---

## 4. Lint calibration — defects and miscalibration in `sc-lint-boundary` (in-workspace)

These 11 findings flag idiomatic Rust the rule cannot currently distinguish from architectural
problems. The non-loosening fix is to make the lint *more precise*, in our own
`crates/sc-lint-boundary` — not to weaken thresholds or baselines. Each change below narrows a
false-positive class while keeping every true-positive test in `tests.rs` green; each must add
pinning tests for the newly excluded shapes.

### 4.1 NodeId collision bug — inherent vs trait method of the same name [1 finding]
- Finding: SCB-CYCLE-002 on `atm-core::service_runtime::LocalServiceRuntime`.
- Root cause (verified against the exported graph): method node ids are
  `{owner_node_id}::{method_ident}` with no impl-block discriminator
  (`graph/ingest.rs:676`), and `add_node` is first-writer-wins (`lib.rs:474-478`). The
  `RetainedServiceRuntime` trait impl's forwarding call
  `Self::load_roster_member(self, …)` (`service_runtime.rs:539`, likewise `:546`) is
  attributed to the *inherent* node of the same name (`:334-338`), manufacturing a fake
  self-loop. No source-code restructure fixes this correctly.
- **Change**: include the impl discriminator (`impl_kind` + `impl_trait` path) in the method
  `NodeId` at `graph/ingest.rs:676`, and derive `source_impl_kind` per-edge from the actual
  originating impl. *(review r1)* the change is wider than ingest: `owner_id_for_node_id`
  (`analysis.rs:472-480`) must be updated to recover the type owner from the new
  impl-qualified method IDs, or every method silently becomes its own owner. Regression tests
  must pin **graph identity** (two distinct method nodes with correct owners, edges attributed
  to the originating impl) **and classification** (no self-loop for the forwarder shape) —
  not merely the absence of the finding.

### 4.2 SCB-CYCLE-002 heuristic — helper delegation is not architectural recursion [7 findings]
- Findings: `LocalCapability` (`local_http.rs:57`), `LocalHttpEndpointRecord`
  (`local_http.rs:105-106`), `PeerCommand` (`commands/peer.rs:125`),
  `ReceiverRecoveryCircuit` (`runtime.rs:99`, `*self = Self::new(now)`),
  `FileIdentity` (`unix_socket.rs:68`, `self == Self::of(metadata)`),
  `SqliteMessageStore` (`lib.rs:318,322,326`, `Self::parse_optional_timestamp`),
  `DecomposedMessageAdmission` (`template_catalog.rs:293`, instance facade over pure static).
  All are the same shape: a method body calling a *sibling associated function of its own
  type* (`Self::helper(...)`) while the signature never names the type. The existing
  `has_expr_ref && !has_type_ref` split (`analysis.rs:149-166`) was already patched
  shape-by-shape (constructor-factory, newtype-factory, signature-only tests) — evidence of
  a known-leaky heuristic.
- **Change** (tightened per review r1): a blanket "associated call ⇒ exclude" rule is not
  implementable or safe as-is — the collector records only Expr/Type reference metadata
  (`reference_collector.rs:23-76`, `lib.rs:397-431`) with no call/callee structure. The fix is
  to **add call-callee metadata** to collected expression references (callee ident + whether
  the path is call-position `Self::`/`OwnType::`), then exclude only *delegation to a
  different associated function of the same owner*. **Direct recursion
  (`Self::same_method()`) must remain a positive.** Required pinning tests: helper-call
  delegation (excluded), bare value use (still positive, existing `tests.rs:809-843` shape),
  direct recursion (still positive), and fully-qualified `OwnType::helper(...)` call
  (excluded).
- Interim (only if the lint fix is deferred): the scoped per-method
  `#[sc_lint(boundary.allow("cycle.type_method_self_loop"))]` attribute exists — but prefer
  fixing the heuristic; 7 attributes on idiomatic code is noise that dilutes the rule.

### 4.3 SCB-CYCLE-003 — port-trait implementations on their own adapter type [3 findings]
- Findings: `CliComposition via AtmGraftClient` (`composition.rs:458-474` — trait fns forward
  to same-named inherent fns that CLI commands also call directly),
  `BoundaryMailStoreView via MailStore` (`legacy_storage_adapters.rs:128-232` — private
  helpers + one trait method composing another; NOTE: verified NOT a Phase-AM deletion
  target — the AM ledger tracks legacy HTTP/peer transport only, this is the Phase-AC storage
  bridge), `SqliteMessageStore via MessageStore` (`lib.rs:355-600` — helpers +
  `save_message_if_absent` composing `self.load_message`, which cannot be removed without
  duplicating the SQL read path).
- These are ports-and-adapters implementations referencing their own type — the dominant
  legitimate pattern for every storage/composer/client trait in this workspace. The rule's
  only escape hatch is the global std-traits ignore list in `config/defaults.toml`, which
  cannot express "this crate's own port trait".
- **Change** (settled per review r1 — the draft's per-trait allowlist option is REJECTED as a
  boundary loosening): reuse 4.2's call-callee metadata and exclude from SCB-CYCLE-003 only
  the self-references that are *proven call-callee delegation* — trait-method-to-trait-method
  composition on `self` and calls to private associated helpers. **Type-position self-edges
  in trait impls remain positives** (they are the genuine layering signal). No trait-wide
  exclusion, no workspace allowlist.
- Pinning tests: the existing inherent+trait dual positives (`tests.rs:1050-1097`) and
  ignored-conversion shape (`tests.rs:1133-1162`) stay as-is; add new tests for the classifier
  boundaries introduced here (delegating trait method excluded; trait method with
  type-position self-reference still positive).

**§4 clears 11 findings.**

---

## Sequencing & validation

1. **Wave 1 (§1 + §2.1 + §2.2)** — one fix worktree off develop (`fix/boundary-regression-easy`),
   one PR to develop. Clears 9–10 findings. Gate: `just lint`, `just test`, `just validate`;
   sc-boundary count must drop from 22 to ≤13 with no new findings.
2. **Wave 2 (§4)** — separate worktree (`fix/sc-boundary-calibration`), since it changes the
   lint itself: 4.1 then 4.2 then 4.3, each with pinning tests; every existing `tests.rs`
   true-positive must stay red where it is red today. Gate: sc-boundary on develop+wave-1
   reports exactly 1 remaining finding (`ack`↔`send`).
3. **Wave 3 (§3.1)** — after design sign-off on the ack/send direction: dedicated worktree,
   dedicated review. Gate: sc-boundary reports 0 findings; `just validate` fully green with
   sc-boundary armed; remove/close AO2-SCBOUNDARY-DEBT-001 waiver and close GH #1028.
4. Throughout: no rule is removed from `just lint all`; no baseline file is introduced; the
   only attributes added are §1.6's two purpose-built recursive-container markers.

## Review history

### Round 1 — arch-ctm critical review (2026-08-26, msg 01M0ZEJN8YY7MAK27JDVA93R43, on 4df997334)
Verdict: **NEEDS PLAN REVISION** — classification agreed, corrections required. All folded
into this revision:
- §1: agreed; added public-API audit for `SearchInput::into_request` (1.1) and
  trait-path tests for `BoundedHostNudgeInjector` (1.5).
- §2.1: agreed conditionally; added the same-counter-after-CAS correctness condition and the
  four required tests (failed-acquire / concurrent-cap / drop-release / budget-drop).
- §2.2: helper list was incomplete (`source_text` lib.rs:183, `hash_api_error` lib.rs:345);
  added exhaustive `Self::` sweep requirement.
- §3.1: **direction changed** — original "move ack types into send" rejected (calls are
  bidirectional: ack/mod.rs:194 vs send/mod.rs:538,579; the move would fake a one-way
  dependency). New direction: narrow sibling write module owning the canonical-write/
  ack-admission contract, both `ack` and `send` depend on it one-way; design review still
  required before dispatch.
- §4.1: root cause confirmed; scope widened to `owner_id_for_node_id` (analysis.rs:472-480)
  and test bar raised to graph-identity + classification pinning.
- §4.2: blanket associated-call exclusion **rejected** (collector lacks call metadata,
  reference_collector.rs:23-76, lib.rs:397-431); replaced with call-callee metadata addition;
  direct recursion `Self::same_method()` stays positive; four classifier-boundary tests added.
- §4.3: trait-wide exclusion and workspace allowlist both **rejected**; narrowed to proven
  call-callee delegation/private helpers only, type-position edges retained as positives.

## Finding-to-section index

| # | Finding | Kind | Section | Difficulty |
|---|---|---|---|---|
| 1 | ack ↔ send | 001 | §3.1 | HARD |
| 2 | LogFieldMap ↔ LogFieldValue | 001 | §1.6 | EASY |
| 3 | SearchInput ↔ SearchRequest | 001 | §1.1 | EASY |
| 4 | teams 9-type family | 001 | §1.2 | EASY |
| 5 | SharedLogBuffer ↔ SharedLogWriter | 001 | §1.3 | EASY |
| 6 | HelperThreadBudget ↔ HelperThreadPermit | 001 | §2.1 | MEDIUM |
| 7 | MessageEnvelope ↔ RawMessageEnvelope | 001 | §1.4 | EASY |
| 8 | LocalCapability | 002 | §4.2 | LINT |
| 9 | LocalHttpEndpointRecord | 002 | §4.2 | LINT |
| 10 | LocalServiceRuntime | 002 | §4.1 | LINT-BUG |
| 11 | PeerCommand | 002 | §4.2 | LINT |
| 12 | SendCommand | 002 | §4.2 (or §2.3) | LINT/MEDIUM |
| 13 | ReceiverRecoveryCircuit | 002 | §4.2 | LINT |
| 14 | FileIdentity | 002 | §4.2 | LINT |
| 15 | SqliteMessageStore (inherent) | 002 | §4.2 | LINT |
| 16 | DecomposedMessageAdmission | 002 | §4.2 | LINT |
| 17 | ScComposeTemplateComposer (inherent) | 002 | §2.2 | MEDIUM |
| 18 | CliComposition via AtmGraftClient | 003 | §4.3 | LINT |
| 19 | BoundedHostNudgeInjector via HostNudgeInjector | 003 | §1.5 | EASY |
| 20 | BoundaryMailStoreView via MailStore | 003 | §4.3 | LINT |
| 21 | SqliteMessageStore via MessageStore | 003 | §4.3 | LINT |
| 22 | ScComposeTemplateComposer via TemplateComposer | 003 | §2.2 | MEDIUM |
