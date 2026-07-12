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
  inventory-only deliverable, but `AD.*` is now permanently consumed by
  unrelated accepted ATM work; no future sc-lint implementation branch is
  authorized until explicit human sign-off is recorded and a new phase
  identifier is assigned

### AD-ISSUE-002 `unix_path_prefixes` Portability-Config Equivalence

- status: open until a new sc-lint execution phase assigns this closure
- owner: unassigned pending new phase allocation
- summary: the vendored analyzer carries `unix_path_prefixes`, but the
  published `sc-lint-portability` surface may not expose an equivalent knob;
  the original sc-lint `AD.4` owner reference is obsolete because accepted
  `AD.4` is unrelated `Reconcile Runtime Removal`. A future sc-lint execution
  sprint must prove direct equivalence, carry the behavior forward in an
  ATM-owned override, or record approved removal

### AD-ISSUE-003 Published-Vs-Vendored Parity Comparison Contract

- status: open until a new sc-lint execution phase assigns this closure
- owner: unassigned pending new phase allocation
- summary: the migration needs one deterministic repo-local parity comparison
  helper and artifact format so boundary and portability cutovers can prove no
  silent rule or JSON-shape regression before deletion begins; the original
  sc-lint `AD.2` / `AD.3` owner references are obsolete because those accepted
  sprint numbers now belong to unrelated caller-context and Claude-retirement
  work

### AD-ISSUE-004 Released `D.1` Dependency-Policy Availability

- status: open until a new sc-lint execution phase assigns this closure
- owner: unassigned pending new phase allocation
- summary: final phase closeout depends on an upstream published `sc-lint`
  `D.1` release that ATM does not control; the checkpoint policy prevents
  silent indefinite carry-forward and caps the phase-local re-review loop at
  two checkpoint cycles before mandatory re-scoping. The original sc-lint
  `AD.9` owner reference is obsolete because accepted `AD.9` is unrelated
  `Update-Member CLI And Roster Repair Path`

### AD-ISSUE-005 Published Rule-Id Continuity For Consumer-Subset Wrappers

- status: open until a new sc-lint execution phase assigns this closure
- owner: unassigned pending new phase allocation
- summary: the retained `sc-lint` docs in this repo do not currently prove
  that published analyzer rule IDs still match ATM's local `PORT-004` /
  `PORT-005` and `SCB-RUNTIME-001` / `SCB-RUNTIME-002` wrapper contracts, so
  each subset-wrapper cutover must prove direct continuity or record an
  explicit upstream-to-ATM mapping. The original sc-lint `AD.4` / `AD.5`
  owner references are obsolete because those accepted sprint numbers now
  belong to unrelated daemon-runtime removal work

## Closed Items

### AD-ISSUE-006 Post-Phase-AD Doc Sync Follow-Up

- status: closed by `SCLINT-PLAN-REFRESH-DEV-1`
- owner: `plan/sc-lint-published-migration`
- summary: this follow-up was folded into the supporting-plan refresh on the
  `plan/sc-lint-published-migration` branch once the accepted `Phase AD` line
  had landed. The refresh:
  - discloses that the original sc-lint `AD.1` through `AD.9` proposal never
    executed
  - records that the accepted ATM `AD.*` namespace was later consumed by
    unrelated caller-identity / post-send work through `AD.35`
  - restores the requirement that any future sc-lint execution line needs
    explicit human sign-off and a new phase identifier
