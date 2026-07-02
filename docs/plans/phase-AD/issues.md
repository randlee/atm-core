# Phase AD Issues Inventory

```yaml
phase: AD
status: active
branch: plan/sc-lint-published-migration
worktree: ../atm-core-worktrees/plan/sc-lint-published-migration
```

This inventory tracks the current known planning and execution risks for the
published `sc-lint` migration proposal.

## Open Items

### AD-ISSUE-001 Proposal Authorization

- status: open until explicit human sign-off
- owner: phase-level planning review
- summary: `Phase AD` is a proposed execution package added after the original
  inventory-only deliverable; no `AD.*` implementation branch is authorized
  until human sign-off is recorded

### AD-ISSUE-002 `unix_path_prefixes` Portability-Config Equivalence

- status: open until `AD.4`
- owner: `AD.4`
- summary: the vendored analyzer carries `unix_path_prefixes`, but the
  published `sc-lint-portability` surface may not expose an equivalent knob;
  `AD.4` must prove direct equivalence, carry the behavior forward in an
  ATM-owned override, or record approved removal

### AD-ISSUE-003 Published-Vs-Vendored Parity Comparison Contract

- status: open until `AD.2` and reused by `AD.3`
- owner: `AD.2`
- summary: the migration needs one deterministic repo-local parity comparison
  helper and artifact format so boundary and portability cutovers can prove no
  silent rule or JSON-shape regression before deletion begins

### AD-ISSUE-004 Released `D.1` Dependency-Policy Availability

- status: open until `AD.9`
- owner: `AD.9`
- summary: final phase closeout depends on an upstream published `sc-lint`
  `D.1` release that ATM does not control; the checkpoint policy prevents
  silent indefinite carry-forward

## Closed Items

- none yet
