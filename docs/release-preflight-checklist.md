# Release Preflight Checklist

This is the operator checklist for ATM release preflight.

Source-of-truth rules:

- use `just validate` as the default local preflight command
- use `.github/workflows/release-preflight.yml` for the CI-owned release gate
- use `scripts/validate_release.py` and `.just/run_lint.py` as the mechanical
  source of truth for what actually runs
- keep detail in scripts; keep this document at the checklist layer

## Default Local Command

```bash
just validate
```

For any release changing daemon, HTTP, TLS, storage write, acknowledgement, or
peer transport, execute the two-host [peer-pair smoke](./peer-pair-smoke.md)
after the normal preflight. Attach its sanitized evidence from both roles to
the release record.

Use the canonical `just smoke` command surface described in
[Smoke testing](./smoke-testing.md); never invoke a smoke Python module
directly.

For a concrete release candidate, the workflow-equivalent local invocation is:

```bash
python3 scripts/validate_release.py all \
  --version <X.Y.Z> \
  --staged-install-root target/phase-ae/staged-install-root \
  --findings release-findings.json
```

## `validate_release.py` Target Inventory

The validator currently accepts these targets:

- `all` — run the full retained release-preflight suite
- `validate` — alias of `all`
- `lint` — run the full repo lint suite via `just lint`
- `support-files` — confirm required release support files exist
- `manifest` — validate release manifest coverage, preflight modes, publish
  order, and staged installed-doc coverage
- `publish-surface` — verify publishable crates/binaries against the manifest
  and release version rules
- `release-binaries` — verify required release binaries are declared
- `inventory` — generate and shape-check release inventory output
- `cargo-lock-drift` — detect release-window `Cargo.lock` drift
- `dependency-currency` — run the generic dependency freshness warning and
  the always-on blocking sc-ecosystem preflight
- `ecosystem-preflight` — run only the blocking sc-ecosystem preflight
- `phase-ad-readiness` — verify the retained Phase AD readiness boundary gates

## Checklist Coverage Matrix

| Checklist item | Coverage | Source |
| --- | --- | --- |
| Run the canonical local preflight command | local | `just validate` |
| Run full validator target set (`all` / `validate`) | local | `scripts/validate_release.py` |
| Run full lint suite | local | `validate_release.py lint` -> `just lint` |
| Verify required release support files exist | local | `validate_release.py support-files` |
| Validate release manifest coverage / preflight modes / publish order | local | `validate_release.py manifest` |
| Validate staged installed-doc membership and entrypoint | local or CI | `validate_release.py manifest` |
| Validate publish surface version and artifact rules | local | `validate_release.py publish-surface` |
| Validate required release binaries (`atm`, `atm-daemon`) | local | `validate_release.py release-binaries` |
| Generate and shape-check release inventory | local | `validate_release.py inventory` |
| Check `Cargo.lock` drift during the release window | local | `validate_release.py cargo-lock-drift` |
| Check generic dependency currency | local | `ATMD_CHECK_DEP_CURRENCY=1 validate_release.py dependency-currency` (warn-only) |
| Check sc-ecosystem currency and integration contracts | blocking local/CI | `validate_release.py ecosystem-preflight` |
| Enforce retained Phase AD readiness gates | local | `validate_release.py phase-ad-readiness` |
| Assert workflow dispatcher ownership (`run_by_agent=publisher`) | CI-only | `.github/workflows/release-preflight.yml` |
| Normalize workflow version input to `vX.Y.Z` / `X.Y.Z` | CI-only | `.github/workflows/release-preflight.yml` |
| Install toolchain / `just` / cargo helpers / `codespell` | CI-only | `.github/workflows/release-preflight.yml` |
| Stage installed docs into deterministic root before validation | CI-only | `.github/workflows/release-preflight.yml` + `scripts/release_artifacts.py stage-install-docs` |
| Upload `release-findings.json` as workflow artifact | CI-only | `.github/workflows/release-preflight.yml` |
| Confirm completed release notes were provided by `team-lead` | agent-specific, not script-covered | `.claude/agents/publisher.md` |
| Download and inspect the workflow `release-findings` artifact after preflight | agent-specific, not script-covered | `.claude/agents/publisher.md` |
| Confirm Homebrew / `winget` prerequisite coordination is in place before publish | agent-specific, not script-covered | `.claude/agents/publisher.md` |

## Blocking sc-ecosystem preflight

Every ATM release must run the following sequence through the
`dependency-currency` target (or directly with the `ecosystem-preflight`
target). The generic registry sweep above remains opt-in and warn-only.

| Dependency | Latest-release lookup and exact pin | Named regression target |
| --- | --- | --- |
| sc-compose | `cargo search sc-composer --limit 1`; update `crates/atm-template-sc-compose/Cargo.toml`'s `sc-composer` pin (and its paired `sc-sha` pin when the release requires it) | `cargo test -p atm-template-sc-compose`, then `sc-compose render --file` against the codex-orchestration and plan-hardening `.j2` fixtures |
| sc-observability | `cargo search sc-observability --limit 1` and `cargo search sc-observability-types --limit 1`; exact-pin both workspace dependencies | `cargo test -p agent-team-mail` (the `crates/atm` package) |
| Wyvern | `gh release list --repo randlee/wyvern --limit 1` (REST fallback: `gh api repos/randlee/wyvern/releases/latest`); update the matching exact `WYVERN_PIN` in `scripts/send-to/atm-send-to.sh` and `.ps1` | `scripts/send-to/probe_wyvern.py` plus `scripts/send-to/run_wyvern_picker.py` (wizard JSON `config` input and `WizardResult.data` unwrap) and the shared `PickerInput`/`PickerOutput` fixture suite |

The Cargo comparison strips a leading `=` from exact pins before comparing
them with crates.io's bare version output. A stale pin, unresolved release,
failed named target, incompatible picker schema, or missing Wyvern executable
blocks release. The missing-binary error is actionable:
`install wyvern before running preflight`. This requirement applies to the
preflight host only; AQ5's `atm` build/test lanes must not acquire Wyvern as a
Cargo or test dependency.

Use this lookup-only transcript command when preparing release evidence:

```bash
python3 scripts/validate_release.py ecosystem-preflight --dry-run \
  --findings target/aq6-ecosystem-preflight-findings.json
```

If a newest upstream release regresses the contract, fix forward when
possible. Otherwise run the validator in the explicitly gated fix-forward
mode, supplying the last-known-good map; the validator rewrites the exact
Cargo/WYVERN pins while preserving manifest and script formatting, reuses the
existing `maybe_file_dep_currency_issue` path, and appends the regression,
pinned-back version, and issue URL to
`docs/plans/phase-aq/evidence/AQ6/ecosystem-preflight.md`:

```bash
ATMD_ECOSYSTEM_FIX_FORWARD=1 \
ATMD_ECOSYSTEM_KNOWN_GOOD='{"sc-composer":"1.4.1","sc-observability":"1.1.0","sc-observability-types":"1.1.0","wyvern":"0.4.0"}' \
ATMD_GH_AUTOFIX_ISSUES=1 \
python3 scripts/validate_release.py ecosystem-preflight \
  --findings target/aq6-ecosystem-preflight-findings.json
```

Without `ATMD_ECOSYSTEM_FIX_FORWARD=1`, the validator reports the regression
and leaves pins unchanged. Manual pin-back is the operator override when the
automated fix-forward mode cannot be used; it must still record the same
tracking issue and evidence before release continues.

## `just lint` Gate Breakdown

`just validate` delegates linting to `just lint`, and `just lint all`
currently gates these 21 subchecks:

- `fmt`
- `clippy`
- `deny`
- `shear`
- `version`
- `identities`
- `lines`
- `boundaries`
- `unix-gating`
- `same-host-portability`
- `runtime-waits`
- `manifests`
- `silent-emit`
- `function-length`
- `legacy-mailbox-paths`
- `capability-degradation`
- `spell`
- `fixed-sleep`
- `ttl-triage`
- `daemon-singleton`
- `pytests`

## Advisory / Manual Lint Lanes

These lint commands exist in the repo surface but are not part of `just lint
all`, and therefore do not currently block `just validate`:

- `just lint modules`
- `just lint sc-boundary`
- `just lint sc-portability`

## CI-Only Additions

These steps happen in the release-preflight workflow but are not part of local
`just validate` execution:

- assert `run_by_agent` is exactly `publisher`
- install the pinned Rust toolchain and required helper tools
- normalize release version input
- create `target/phase-ae/staged-install-root`
- stage installed docs into that deterministic root
- pass `--version` and `--staged-install-root` to the validator
- upload `release-findings.json` for later inspection

## Publisher-Specific Manual Checks

These remain part of release preflight discipline even though `just validate`
does not execute them:

- obtain completed release notes from `team-lead` before release execution
- after workflow completion, download the `release-findings` artifact and read
  `release/release-findings.json`
- if the findings artifact reports blockers, stop and report the full blocker
  set to `team-lead`
- verify manual handoff prerequisites for Homebrew or `winget` when automation
  depends on external systems

## Usage Notes

- use this document for sequencing and scope
- use `just validate`, `.github/workflows/release-preflight.yml`, and
  `scripts/validate_release.py` for exact mechanics
- if this checklist disagrees with the scripts, fix this document or the
  scripts immediately; do not preserve conflicting live guidance
