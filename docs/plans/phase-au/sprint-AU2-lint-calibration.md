# Sprint AU.2 — sc-lint-boundary Calibration (mid wave)

status: proposed
assignee: arch-ctm
difficulty: mid
branch: feature/pau-s2-lint-calibration (off integrate/phase-au)
pr_target: integrate/phase-au
parallel_safe: AU.1, AU.3 (this sprint touches crates/sc-lint-boundary ONLY)
master_plan: [boundary-regression-plan.md](../boundary-regression-plan.md) §4.1, §4.2, §4.3

## Scope

Clear 11 findings (index #8–11, #13–16, #18, #20, #21; also #12 unless AU.1 takes its
optional item 9) by making the lint **more precise** — narrowing three verified
false-positive classes. This is calibration, not loosening: no rule is removed, no
threshold weakened, no baseline added, and every existing true-positive test in
`crates/sc-lint-boundary/src/tests.rs` must stay red where it is red today. The
technical specifications (post-review-r1 versions) are authoritative in the master plan.

Work items, in-sprint order (4.2 and 4.3 share the new metadata; 4.1 changes node
identity that their tests reference — implement 4.1 first):

1. **§4.1 NodeId impl-discriminator (clears #10)** — method `NodeId` at
   `graph/ingest.rs:676` gains the impl discriminator (`impl_kind` + `impl_trait`
   path); `owner_id_for_node_id` (`analysis.rs:472-480`) updated to recover the type
   owner from impl-qualified method IDs; per-edge `source_impl_kind` derived from the
   actual originating impl. Root cause is the verified first-writer-wins collision
   (`lib.rs:474-478`) merging same-named inherent + trait methods.
2. **§4.2 call-callee metadata + helper-delegation exclusion (clears #8,9,11,13,14,15,16
   and #12 unless AU.1 took it)** — add callee metadata to collected expression
   references (`reference_collector.rs:23-76`, `lib.rs:397-431`); SCB-CYCLE-002 then
   excludes only *delegation to a different associated function of the same owner*.
   **Direct recursion `Self::same_method()` remains a positive.**
3. **§4.3 narrowed trait-impl classifier (clears #18, #20, #21)** — reuse the call-callee
   metadata; SCB-CYCLE-003 excludes only proven call-callee delegation
   (trait-method-to-trait-method composition on `self`, calls to private associated
   helpers). **Type-position self-edges in trait impls remain positives.** The
   trait-wide exclusion and workspace allowlist options were reviewed and REJECTED —
   do not implement either.

## Test deliverables (from the 2026-08-26 coverage audit)

The suite currently pins classification outcomes only; graph identity has zero coverage
beyond a trivial `NodeId::new("")` panic test. Required new pinning tests:

- **§4.1 graph identity**: same-named inherent + trait-impl methods produce two distinct
  method nodes with correct owners; edges attributed to the originating impl; the
  forwarder shape (`Self::name(self, ..)` inside the trait impl) produces no self-loop.
  Pin identity AND classification — not merely the absence of the finding.
- **§4.2 classifier boundaries**: helper-call delegation excluded; bare value use still
  positive (existing tests.rs:809-843 shape retained); direct recursion still positive;
  fully-qualified `OwnType::helper(...)` call excluded.
- **§4.3 classifier boundaries**: delegating trait method excluded; trait method with a
  type-position self-reference still positive. Existing dual-positive
  (tests.rs:1050-1097) and ignored-conversion (tests.rs:1133-1162) tests stay as-is.

## Acceptance criteria

- The owned findings are absent from the sc-boundary full JSON payload run against the
  application crates, verified by finding identity; no new findings introduced; findings
  owned by AU.1/AU.3 untouched (in particular, ack↔send #1 must still be reported —
  proof the calibration did not over-exclude).
- Every pre-existing tests.rs true-positive still fails/reports exactly as before.
- All new pinning tests present and passing; changes confined to
  `crates/sc-lint-boundary/`.
- `just test`, `just lint` pass.

## Validation

`cargo test -p sc-lint-boundary`; `just test`; sc-boundary full-payload diff against the
22-finding baseline showing exactly the owned findings removed and #1 retained.
