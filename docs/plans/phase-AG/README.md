# Phase AG

Phase `AG` owns completion of the Windows/macOS cross-host product surface and
the validation needed to declare it release-usable.

This phase no longer assumes the intended cross-host interfaces already exist.
Work completed so far proved the opposite: the original `1.3.1` cross-host line
was missing durable interface selection, inbound host authorization, and
operator-visible diagnostics. Phase `AG` now treats those as prerequisite
product work before live cross-host validation can close for real.

Current phase framing:

- `AG.1` and `AG.2` capture the setup/runbook work and the first real
  cross-host attempts that exposed the missing control-plane surface
- `AG.3` captures the daemon loopback self-test surface that now exists and
  remains part of the supported product contract
- later AG sprints add the missing durable control plane:
  - SQLite-backed interface configuration
  - SQLite-backed inbound host allowlist enforcement
  - CLI administration for both
  - `atm doctor` visibility for both
- corrective downstream sprints after the reviewed AG.6-AG.10 line then close
  the still-missing remote-target dispatch gap and revalidate the full feature
  ladder in this order:
  - localhost same-host remote-target proof
  - self-IP same-host remote-target proof
  - automated integration coverage
  - other-Mac smoke
  - Windows/macOS smoke
  - copied-state final verdict
- TLS / transport security remains a late AG concern and must not be implied by
  earlier functional cross-host closure

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
- `sprint-AG6.md`
- `sprint-AG7.md`
- `sprint-AG8.md`
- `sprint-AG9.md`
- `sprint-AG10.md`
- `sprint-AG11.md`
- `sprint-AG12.md`
- `sprint-AG13.md`
- `sprint-AG14.md`
- `sprint-AG15.md`
- `sprint-AG16.md`
- `sprint-AG17.md`

Historical input:

- `docs/plans/phase-AB/`

`Phase AB` remains the earlier cross-host smoke planning line, but it never
reached executed readiness evidence and is retained only as historical planning
input. `Phase AG` reuses the useful structure from `AB`, supersedes it as the
active namespace, and is the only current release-directed validation package.
