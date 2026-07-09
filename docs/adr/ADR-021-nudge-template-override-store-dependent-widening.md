# ADR-021 — NudgeTemplateOverrideStore Dependent Widening

| Field | Value |
|---|---|
| ID | ADR-021 |
| Status | **Accepted** |
| Date | 2026-07-09 |
| Deciders | Rand Lee |
| Relates to | ADR-001, ADR-020, ARCH-001, ARCH-002 |
| Supersedes | — |

---

## Context

`AD.25` introduced live built-in nudge override flows that made two retained
compile-bridge dependents real consumers of the
`NudgeTemplateOverrideStore` boundary:

- `atm-daemon-bootstrap`
- `atm`

That implementation work was correct, but the original
`boundaries/atm-core/nudge-template-override-store.toml` still allowed only:

- `atm-runtime`
- `atm-storage-rusqlite`

It also still carried a now-contradictory forbidden reference to
`atm::commands::internal_nudge`.

`ARCH-001` closed the machine-readable TOML side by widening
`allowed_dependents` and removing the contradictory forbidden reference.
`ARCH-002` then confirmed the human-readable boundary record must carry the
same relaxation path explicitly under `RULE-012`, using a concrete decision
record rather than informal message provenance only.

The accepted precedent already existed in
`boundaries/atm-core/roster-store.toml`, which documents the same dependent
shape on the retained Phase AD line.

## Decision

Accept the `NudgeTemplateOverrideStore` boundary-dependent widening on the
Phase AD line:

- `atm-daemon-bootstrap` is an allowed dependent of
  `NudgeTemplateOverrideStore`
- `atm` is an allowed dependent of `NudgeTemplateOverrideStore`
- the stale forbidden reference to `atm::commands::internal_nudge` is removed
  because it directly contradicts the accepted dependent shape

This widening is accepted specifically because:

- the live retained bootstrap/CLI path implemented in `AD.25` genuinely uses
  this storage-neutral contract
- the dependency shape mirrors the already-accepted `RosterStore` precedent
- the change narrows to compile-bridge dependency bookkeeping only; it does not
  relax direct SQLite access or move template-rendering ownership out of `atm`

## Enforcement

This ADR is valid only while both boundary records stay aligned:

- `boundaries/atm-core/nudge-template-override-store.toml` must keep
  `atm-daemon-bootstrap` and `atm` in `allowed_dependents`
- `docs/atm-core/boundaries.md` must describe the same widening and cite this
  ADR explicitly

Review and lint expectations remain:

- machine-readable boundary TOML is still the enforcement source
- the human-readable boundary record must be updated in the same relaxation
  path so `RULE-012` remains satisfied
- no new dependent may be added later without repeating the same documented
  relaxation path

## Boundary Conditions

This ADR does not relax any other boundary:

- it does not authorize direct SQLite access from `atm` or
  `atm-daemon-bootstrap`
- it does not authorize emitter-side lookup or template selection inside
  `PostSendHookEmitter`
- it does not reopen daemon-side ownership of built-in template rendering
- it does not widen any other `atm-core` storage boundary by implication

## Consequences

### Positive

- the machine-readable and human-readable `NudgeTemplateOverrideStore` boundary
  records are now consistent
- the retained AD.25 compile-bridge implementation matches the documented
  boundary contract truthfully
- future review has a concrete decision record instead of relying on ATM
  message archaeology

### Negative

- the boundary is less restrictive than its original narrow storage-only shape
- one more ADR now exists to govern a Phase AD-specific compile-bridge
  exception pattern

## Review Conditions

This ADR remains acceptable only while all of the following stay true:

- `atm-daemon-bootstrap` and `atm` consume the storage-neutral override-store
  contract rather than bypassing it
- `atm` remains an allowed dependent because team-admin commands such as
  `atm teams set-nudge-template` consume the contract; retained
  `atm::commands::internal_nudge` helper code is no longer a direct contract
  consumer
- the dependency shape continues to match the retained `RosterStore`
  precedent rather than drifting into broader boundary leakage

If those conditions stop being true, this ADR must be revisited rather than
silently widening or narrowing the boundary record.
