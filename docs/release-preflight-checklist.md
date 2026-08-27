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
- `dependency-currency` — check dependency freshness
- `phase-ad-readiness` — an explicit historical diagnostic used by the
  thorough smoke lane; it is no longer part of the default release target

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
| Check dependency currency | local | `validate_release.py dependency-currency` |
| Run the optional historical Phase AD diagnostic | thorough-smoke only | `validate_release.py phase-ad-readiness` |
| Assert workflow dispatcher ownership (`run_by_agent=publisher`) | CI-only | `.github/workflows/release-preflight.yml` |
| Normalize workflow version input to `vX.Y.Z` / `X.Y.Z` | CI-only | `.github/workflows/release-preflight.yml` |
| Install toolchain / `just` / cargo helpers / `codespell` | CI-only | `.github/workflows/release-preflight.yml` |
| Upload `release-findings.json` as workflow artifact | CI-only | `.github/workflows/release-preflight.yml` |
| Confirm completed release notes were provided by `team-lead` | agent-specific, not script-covered | `.claude/agents/publisher.md` |
| Download and inspect the workflow `release-findings` artifact after preflight | agent-specific, not script-covered | `.claude/agents/publisher.md` |
| Confirm Homebrew / `winget` prerequisite coordination is in place before publish | agent-specific, not script-covered | `.claude/agents/publisher.md` |

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
- pass `--version` to the validator
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
