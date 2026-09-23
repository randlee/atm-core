---
status: complete
branch: feature/bc1-sc-observability-1-4-0
worktree: /Users/randlee/github/atm-core-worktrees/feature/bc1-sc-observability-1-4-0
---

> **Superseded by bc.6 for current release qualification.** The frozen
> 1.4.0 record remains historical evidence. ATM cfg(test) fault injection
> exercised the retained sink health seam, but did not exercise upstream true
> queue saturation. The CI receipt boundary for this record is
> `1a7c50a1a59afc3ff28edbc8acf5b4391440dd14`.

# sprint bc.1 — qualify published sc-observability 1.4.0

Accountable owner: `arch-ctm@atm-dev`.

## goal

Establish the lowest-risk compile and behavior baseline against the published
`1.4.0` family without adopting new logging ownership or changing ATM public
contracts.

## deliverables

1. Change exact workspace pins for `sc-observability` and
   `sc-observability-types` from `=1.2.0` to `=1.4.0`; update `Cargo.lock`.
2. Add an ATM consumer fixture that compiles the current logger configuration,
   retained policy, query, health, log, flush, shutdown, and fault-injection
   paths against the exact published crates.
3. Record the resolved crate versions and checksum/source identities in
   `docs/plans/phase-bc/bc.1-1.4.0-qualification.md`.
4. Make only compatibility edits required to preserve existing ATM behavior.
5. Update current normative version references, including
   `docs/requirements.md`, `docs/atm-daemon/logging.md`, and the ecosystem
   known-good release-preflight inventory. Preserve explicitly historical
   phase and release records rather than rewriting their old baselines.

## acceptance

- `cargo tree` resolves the intended direct crates at exactly `1.4.0` with no
  duplicate `sc-observability-types` generation.
- Existing retained JSONL, doctor projection, queue-full, flush, shutdown,
  query, and fault-injection tests pass unchanged in meaning.
- Linux, macOS, and Windows checks compile the consumer fixture.
- No dependency on nonexistent `sc-observability-tokio` is introduced.
- No `sc-observability-log` global initialization or macro migration occurs.
- No current normative document or release-preflight inventory continues to
  prescribe an older sc-observability family version.
- The qualification artifact contains the exact lockfile identities, published
  source/checksum evidence, validation results, and any compatibility edits.

## required validation

Run workspace format, clippy, tests, CLI-surface, boundary, function-length,
nudge-taxonomy, concurrency, identity, ADR-index, and manifest gates from the
phase stack guidelines. Add an offline/extracted-package consumer check when
the published archive is available locally.

## out of scope

Typed cleanup, macro adoption, bridge replacement, release publication, and
the future `1.4.1` repin.
