# Boundary Regression Fix Plan — the 22 sc-boundary Findings

status: draft_for_review
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
- **Proposed direction** (needs sign-off before dispatch): relocate the write-pipeline-shaped
  ack types — `AckOutcome`, `AckReplyDisposition`, `ResolvedAcknowledgement`,
  `AtomicAcknowledgementWrite`, `ReplyTarget` — into `send` (or a `send::ack_outcome`
  submodule), with `pub use` re-exports from `ack` to preserve the public path surface.
  `ack` retains `AckRequest`, `ack_mail[_with_runtime]`, and
  `admit_acknowledgement_write[_async]`, importing the moved types — making the dependency
  one-way (`ack → send`).
- Open questions for the design discussion: (a) do ack-outcome types *belong* to the write
  pipeline (send) or should a third module own the shared write/outcome contract with both
  `ack` and `send` depending on it one-way? (b) serde/public-API compatibility of the moved
  types (re-exports should preserve paths, but verify against atm-graft/daemon consumers).
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
  originating impl. Add a regression test: inherent method + same-named trait-impl forwarder
  must not self-loop.

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
- **Change**: in the reference collector / classifier, treat an expression reference that is a
  **call to an associated function of the same owner** (path of the form
  `Self::ident(...)` / `OwnType::ident(...)` in call position) as helper delegation, not a
  self-loop trigger. Keep flagging non-call expression uses (bare `Loop;` value uses, the
  original motivating case). Pin with tests for each of the 7 shapes above plus the existing
  negative tests.
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
- **Change** (design decision, pick one — recommend (a)):
  - (a) Exclude from SCB-CYCLE-003 the self-references that are *trait-method-to-trait-method
    composition on `self`* and *calls to private associated helpers* (same delegation logic
    as 4.2), leaving the rule to fire on type-position self-references in trait impls that
    indicate genuine layering violations.
  - (b) Add a workspace-config (not embedded-defaults) ignored-traits extension so a repo can
    declare its own port traits (`MessageStore`, `TemplateComposer`, `AtmGraftClient`,
    `MailStore`, `HostNudgeInjector`) — more explicit, but is a per-trait allowlist and
    needs governance to avoid becoming a dumping ground.
- Pin with tests either way.

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
