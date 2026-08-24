# Sprint AQ6 — SC-Ecosystem Dependency Preflight and Wyvern Contract Issue

Status: draft · Branch: `feature/aq-6-ecosystem-preflight` off
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

1. **Preflight dependency rules**: extend `docs/release-preflight-checklist.md`
   (and the `preflight` skill/publisher flow that executes it) with a
   mandatory step for every atm release: for each sc-ecosystem dependency
   (sc-compose, sc-observability, Wyvern — one authoritative list, extend
   here as the ecosystem grows), (a) look up the most recent release,
   (b) bump the recorded pin to it, (c) run the dependency's integration
   tests against that release (for Wyvern: the AQ5 picker fixture suite —
   `PickerInput`/`PickerOutput` `schema_version`, cancel semantics,
   cold-start measurement — against the real binary). A regression found
   here **blocks the atm release until fixed forward** (ours or an upstream
   issue) — the answer is never staying on an old pin.
2. **Wyvern pin-bump mechanics**: the AQ5 pin constant is the single source
   the preflight step updates; the preflight verifies pinned == latest
   available and that the pinned Wyvern supports the expected picker
   `schema_version`. Documented in the checklist with the exact commands.
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

1. `docs/release-preflight-checklist.md` (and the executing skill) carry the
   sc-ecosystem bump-to-latest + integration-test step with the
   fix-forward rule; the dependency list names sc-compose,
   sc-observability, and Wyvern.
2. A dry-run of the preflight step against current releases is executed
   once and its transcript committed as evidence (proves the mechanics,
   whatever the result).
3. The Wyvern GH issue exists, covers every contract element listed in
   deliverable 3, and its URL is recorded in the phase evidence register.
4. `just test` unaffected; all three CI lanes green.
5. **Phase closure**: the `integrate/phase-aq` → `develop` merge PR is
   opened from the final integrate head (all six sprints merged) with the
   AQ5 evidence file and this sprint's issue URL linked; full `just test` +
   integration suites green on that head.

## Paths to delete

None.

## Required validation

- Preflight dry-run transcript committed on the branch.
- quality-mgr review of the checklist step and the issue text against the
  PRD contract sections.

## Non-closure / out of scope

- Implementing Wyvern-side tests (upstream work, tracked by the issue).
- Auto-bump tooling beyond the documented preflight step (follow-on if the
  manual step proves error-prone).

## Dependencies

- must_follow: AQ5 (the pin constant, fixture corpus, and picker contract
  must be final before the preflight rules and upstream issue cite them).
- parallel_safe: none remaining.
