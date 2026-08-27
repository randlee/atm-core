# Sprint AQ6 — SC-Ecosystem Dependency Preflight and Wyvern Contract Issue

Status: complete · Branch: `feature/aq-6-ecosystem-preflight` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Closes the phase's ecosystem-integration loop (Rand, 2026-08-23): atm always
tracks the most recent releases of its sc-ecosystem dependencies —
sc-compose, sc-observability, Wyvern — with preflight rules that bump pins
to latest and prove no regression at every atm release, instead of letting a
"supports everything after X" range grow the integration surface. Also files
the upstream contract-test request so the atm-core picker contract is
regression-tested inside Wyvern's own CI.

## Deliverables

1. **Preflight dependency rules**: extend the **existing
   `dependency-currency` validator target**
   (`validate_dependency_currency`, `scripts/validate_release.py:646-690`)
   in `docs/release-preflight-checklist.md` and the `preflight` skill/
   publisher flow that executes it. Today that target is **opt-in**
   (skipped unless `ATMD_CHECK_DEP_CURRENCY=1`, the env var named at
   `validate_release.py:35`, not merely "set" — the constant's own name is
   `CHECK_DEP_CURRENCY_ENV`), **warn-only** (every finding is
   `severity="warning"`, never blocking), and **registry-only** (`cargo
   search` against the default registry via `latest_registry_version`,
   `validate_release.py:610` — it cannot see an arbitrary `PATH`
   binary like `wyvern`, which is not a Cargo dependency at all, and would
   not distinguish the sc-ecosystem crates from any other stale dependency
   in the workspace). This deliverable adds a **new, always-on, sc-
   ecosystem-scoped step** in the same checklist section — extending the
   target's *scope*, not its existing opt-in/warn-only registry behavior,
   which stays as-is for the general dependency sweep — with a mandatory
   step for every atm release: for each sc-ecosystem dependency
   (sc-compose, sc-observability, Wyvern — one authoritative list, extend
   here as the ecosystem grows), (a) look up the most recent release,
   (b) bump the recorded pin to it, (c) run that dependency's named
   integration-test target against the bumped release:
   - **Wyvern** (target repo approved by Rand 2026-08-26): looked up via `gh release list --repo randlee/wyvern
     --limit 1` (the real Wyvern repo — confirmed via existing cross-repo
     references, e.g. `randlee/wyvern#115` cited in
     `docs/plans/phase-ao2/benchmark-reporting-plan-overview.md:36` and
     `sprint-AO2-11-benchmark-rendering-pipeline.md:59`) (the preflight script already shells out to
     `gh`, precedent: `maybe_file_dep_currency_issue`,
     `validate_release.py:622`, which calls `gh issue create`) since
     Wyvern has no crates.io registry entry for `cargo search` to find;
     verified present via a `wyvern --version` `PATH` probe (same
     bounded-deadline shape as AQ5 deliverable 3a's runtime probe, reused
     here rather than redefined) rather than `latest_registry_version`.
     **Prerequisite**: the preflight host itself must have `wyvern`
     installed on `PATH` to run this step — an operational requirement of
     running the *full* preflight, distinct from AQ5 3a's guarantee that
     no `atm` build or test lane requires Wyvern; a preflight host missing
     `wyvern` fails this step with an actionable "install wyvern before
     running preflight" error, not a silent skip. Regression check: the
     AQ5 picker fixture suite (`PickerInput`/`PickerOutput`
     `schema_version`, cancel semantics, cold-start measurement) against
     the real binary;
   - **sc-compose**: pinned via the Cargo-level `sc-composer` dependency,
     which crates.io registry lookup already covers (it is already
     exact-pinned, `sc-composer = "=1.4.1"`,
     `crates/atm-template-sc-compose/Cargo.toml:18` — the precedent this
     deliverable extends to the other two Cargo-level sc-ecosystem deps
     below). Regression check: `cargo test -p atm-template-sc-compose`
     plus an `sc-compose render` smoke over the repo's canonical `.j2`
     assets (the codex-orchestration / plan-hardening templates) with the
     bumped binary;
   - **sc-observability**: also Cargo-level and registry-visible, but
     currently **caret-ranged**, not exact-pinned — `sc-observability =
     "1.2.0"` and `sc-observability-types = "1.2.0"` at
     `Cargo.toml:44-45`. This deliverable changes both to `"=1.2.0"`
     (matching the `sc-composer` precedent above) so `cargo search`'s
     registry lookup and the recorded pin mean the same exact version, not
     "1.2.0 or any compatible later 1.x." **Latent comparison bug this
     deliverable must also fix**: `direct_registry_dependencies`
     (`validate_release.py:566`) records the literal manifest string as
     `current` (so, after this change, `"=1.2.0"`), but
     `latest_registry_version`'s `cargo search` output is always a bare
     version (`"1.2.0"`, no `=`); the `latest != current` check at
     `validate_release.py:672` would then report every correctly-current
     exact-pinned dependency — including the pre-existing `sc-composer
     = "=1.4.1"`, which has never actually been checked since nobody has
     run `ATMD_CHECK_DEP_CURRENCY=1` since that pin landed — as
     permanently stale. Fix: strip a leading `=` from `current` before the
     comparison (and in the "stale" `Finding` detail string, so the
     reported current value matches `Cargo.toml`). Regression check: **not** the
     legacy synchronous daemon's `crates/atm-daemon/bin_support/
     daemon_observability.rs` tests (off-limits per CLAUDE.md, and its
     `emit_daemon_event` is `pub(crate)` to that legacy binary, due for
     wholesale deletion in Phase AM — it cannot be this deliverable's
     precedent or its regression target). `crates/atm-core/src/log/mod.rs`
     only comments on the `sc-observability` integration (no `use
     sc_observability` import — verified, grep is empty there); the one
     **real, non-legacy** direct call site is `crates/atm/src/main.rs`
     (61 references — the CLI's JSONL log sink, retention/rotation policy,
     and health-state mapping — with its own `#[cfg(test)]` module from
     line 1010 exercising `sc_observability_types` level/health mapping).
     Regression check: `cargo test -p atm` (that module), built against
     the bumped release. `atm_core::observability::ObservabilityPort` (the
     replacement-runtime event-emission seam, consumed via
     `crates/atm-daemon-bootstrap/src/lib.rs:463` and
     `crates/atm-http-runtime/src/storage_and_nudge_router.rs:95`) is an
     internal abstraction over structured events and does not itself
     import the `sc_observability` crate — it is not this deliverable's
     regression target.
   Where a listed target proves insufficient, this deliverable creates the
   missing coverage under `.just/tests/test_ecosystem_pins.py` rather than
   leaving the step unfalsifiable. A regression found in this **sc-
   ecosystem step blocks the atm release until fixed forward** (severity
   distinct from the pre-existing generic warn-only sweep) — ours or an
   upstream issue, the answer is never silently staying on an old pin.
   **Fix-forward escape hatch**: when the fix must land upstream and can't
   land before this release, the preflight step is allowed to pin back to
   the last known-good version instead of blocking indefinitely, but only
   paired with a filed, linked tracking issue — reusing and extending the
   existing `maybe_file_dep_currency_issue`/`ATMD_GH_AUTOFIX_ISSUES=1`
   mechanism (`validate_release.py:622`) rather than inventing a
   second issue-filing path — recording the regression, the pinned-back
   version, and the issue URL in the phase evidence register (AQ5
   deliverable 6) so a pinned-back release is visible, not silent.
2. **Wyvern pin-bump mechanics**: the AQ5 pin constant is the single source
   the preflight step updates; the preflight verifies pinned == latest
   available (via the `gh release list` lookup above) and that the pinned
   Wyvern supports the expected picker `schema_version`. Documented in the
   checklist with the exact commands.
3. **Detailed GitHub issue on the Wyvern repo** requesting contract
   regression tests in Wyvern CI, specifying: the `PickerInput`/
   `PickerOutput` JSON schemas with `schema_version` semantics
   (PRD §4.2/§5a verbatim), stdin/stdout discipline (single JSON object
   out, nothing else on stdout), cancel = nonzero exit with no output,
   `--version` reporting contract (parseable, fast, used by the atm probe),
   the ~1 s launch-to-interactive budget, and the shared fixture corpus
   location so both repos test the same bytes. The issue is written so
   Wyvern maintainers can implement without reading atm-core source; its
   URL is recorded in the phase evidence (AQ5 deliverable 6's register).

## Acceptance criteria

1. The `dependency-currency` target in `docs/release-preflight-checklist.md`
   (and the executing skill) carries the sc-ecosystem bump-to-latest +
   integration-test step with the fix-forward rule; the dependency list
   names sc-compose, sc-observability, and Wyvern, each with its concrete
   integration-test target from deliverable 1; the pre-existing opt-in/
   warn-only registry-wide `dependency-currency` check
   (`ATMD_CHECK_DEP_CURRENCY=1`) is unchanged and unaffected — this
   sprint adds an always-on, blocking, sc-ecosystem-scoped step alongside
   it, not a replacement. `sc-observability`/`sc-observability-types` in
   root `Cargo.toml` are exact-pinned (`"=1.2.0"`, matching the existing
   `sc-composer = "=1.4.1"` precedent) instead of caret-ranged.
1a. A preflight host missing the `wyvern` binary on `PATH` fails the
   Wyvern currency step with a named, actionable error distinct from a
   currency regression — proving the "Wyvern-on-preflight-host"
   prerequisite is enforced, not merely documented — while AQ5 3a's
   guarantee (no `atm` build/test lane requires Wyvern) remains
   independently verified and unaffected by this new preflight-host
   requirement.
   The two platform Send-To entry points retain paired `WYVERN_PIN`
   declarations; the preflight treats them as one logical source and fails
   actionably if either declaration is missing, duplicated, or disagrees with
   the other.
1b. The fix-forward escape hatch: a simulated upstream regression (stale
   fixture/stub release) drives the preflight step to pin back to the
   last known-good version, file (or reuse, if `ATMD_GH_AUTOFIX_ISSUES=1`
   is unset in the test, record-only) a tracking issue via the extended
   `maybe_file_dep_currency_issue`, and record both the pinned-back
   version and the issue reference in the phase evidence register —
   proving a single upstream break cannot block every subsequent atm
   release indefinitely.
2. A dry-run of the preflight step against current releases is executed
   once and its transcript committed as evidence (proves the mechanics,
   whatever the result).
3. The Wyvern GH issue exists, covers every contract element listed in
   deliverable 3, and its URL is recorded in the phase evidence register.
4. `just test` unaffected; all three CI lanes green.
5. **Phase closure**: the `integrate/phase-aq` → `develop` merge PR is
   opened from the final integrate head (**every sprint row in the plan
   table merged — fourteen as of 2026-08-26, including all AQ1.x/AQ2.x
   insertion sprints**; closure is not declarable with any row unmerged) with the
   AQ5 evidence file and this sprint's issue URL linked; full `just test` +
   integration suites green on that head.

## Paths to delete

None.

## Required validation

- Preflight dry-run transcript committed on the branch.
- quality-mgr review of the checklist step and the issue text against the
  PRD contract sections.

## Acceptance evidence

| Criterion | Evidence/status |
| --- | --- |
| AC1 / AC1a / AC1b | Blocking validator, exact pin comparison, actionable Wyvern prerequisite, paired `WYVERN_PIN` single-source check, gated pin-back mutation, regression tests, and evidence-register recording in `scripts/validate_release.py` and `.just/tests/test_ecosystem_pins.py`. |
| AC2 | Dry-run transcript committed in `docs/plans/phase-aq/evidence/AQ6/ecosystem-preflight.md`. |
| AC3 | Wyvern contract issue [#139](https://github.com/randlee/wyvern/issues/139), linked from the AQ6 evidence register. |
| AC4 | PR #1066 CI lane run IDs are recorded in the evidence file after the final checks complete; local lint/test phases passed, with local macOS codesign requiring an interactive keychain and therefore not claimable here. |
| AC5 | Verified at Phase AQ closeout (integrate→develop PR) — not claimable in this PR. |

## Non-closure / out of scope

- Implementing Wyvern-side tests (upstream work, tracked by the issue).
- Auto-bump tooling beyond the documented preflight step (follow-on if the
  manual step proves error-prone).

## Dependencies

- must_follow: AQ5 (the pin constant, fixture corpus, and picker contract
  must be final before the preflight rules and upstream issue cite them).
- parallel_safe: none remaining.
