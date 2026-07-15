# Phase AG

Phase `AG` owns Windows/macOS cross-host interface validation for the `1.3.1`
release line.

This is a validation-first phase:

- assume the intended cross-host interfaces already exist
- prove they work on real binaries before authorizing any code change
- treat every failure as a concrete product finding with exact reproduction
- keep code changes out of scope unless the validation matrix exposes a real bug
- do not certify cross-host release-usability while the transport-security
  requirement remains unverified or knowingly violated

Planning source of truth:

- [`plan-phase-AG.md`](./plan-phase-AG.md)

Expected planning artifacts:

- `plan-phase-AG.md`
- `cross-host-setup-runbook.md`
- `cross-host-smoke-checklist.md`
- `cross-host-findings-ledger.md`
- `readiness.md`
- `sprint-AG1.md`
- `sprint-AG2.md`
- `sprint-AG3.md`
- `sprint-AG4.md`
- `sprint-AG5.md`

Historical input:

- `docs/plans/phase-AB/`

`Phase AB` remains the earlier cross-host smoke planning line, but it never
reached executed readiness evidence and is retained only as historical planning
input. `Phase AG` reuses the useful structure from `AB`, supersedes it as the
active namespace, and is the only current release-directed validation package.
