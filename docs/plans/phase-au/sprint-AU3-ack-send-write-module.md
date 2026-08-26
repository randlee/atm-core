# Sprint AU.3 — ack/send Shared Write Module (hard wave)

status: proposed
assignee: fenix (team-lead)
difficulty: hard
branch: feature/pau-s3-ack-send-write-module (off integrate/phase-au)
pr_target: integrate/phase-au
parallel_safe: AU.1, AU.2 (touches only `atm-core` ack/send + new write module, and the
atm-architecture tripwire test — disjoint from both other sprints' file sets)
master_plan: [boundary-regression-plan.md](../boundary-regression-plan.md) §3.1

## Scope

Retire the one **true** boundary violation: the bidirectional `ack` ↔ `send` module
cycle (finding index #1; SCB-CYCLE-001 multi-owner SCC). Per arch-ctm's round-1
recommendation, resolve it with a **sibling write module** (working name
`atm_core::write`) that both `ack` and `send` depend on — neither imports the other
afterward. The calls are genuinely bidirectional today (`ack/mod.rs:194` →
`send::write_mail_with_runtime`; `send/mod.rs:538,579` → `ack::admit_acknowledgement_write`),
so moving ack types into send was reviewed and rejected.

**Mechanical relocation only** (benchmark constraint): items move with unchanged
signatures, unchanged bodies, and no new indirection (no traits, no dyn, no wrapper
functions) on the write hot path. Serde shapes and any externally visible paths are
preserved via `pub use` re-exports from the original modules.

## Step 0 — design confirmation (internal precondition; does not block AU.1/AU.2)

Confirm the write-module member set with arch-ctm before any code moves. Open question
from the master plan: types only, or types + admission functions. Working position
(fenix): the admission functions **must** move — leaving `admit_acknowledgement_write`
in `ack` while `write_mail_with_runtime` moves would keep one leg of the cycle. Record
the confirmed member list in this doc before starting step 2.

## Work items (in order)

1. **Entry tests first** — the admission flow currently has exactly one end-to-end test
   (async variant, in atm-http-runtime); the sync `admit_acknowledgement_write` has no
   dedicated atm-core test. BEFORE any relocation, add direct atm-core unit tests for
   both `admit_acknowledgement_write` and `admit_acknowledgement_write_async` pinning
   current behavior (admit, reject-reacknowledge, and error paths). These are the
   refactor's regression net.
2. **Create `atm_core::write`** with the step-0-confirmed member set, moved verbatim.
3. **Re-point `ack` and `send`** to depend on `write`; remove all `crate::ack::` refs
   from send and `crate::send::` refs from ack. Add `pub use` re-exports in `ack`/`send`
   for any moved items that are public API or named in serde/persisted paths.
4. **Re-point the tripwire** — `atm-architecture/tests/boundary_enforcement.rs:362-406`
   (`acknowledgement_cannot_restore_a_second_write_pipeline`) greps ack/send source for
   the current cross-module call strings. Update it to assert the NEW invariant (all
   mail-write admission flows through `crate::write::…`; no `crate::send::write_…` /
   `crate::ack::admit_…` cross-calls reappear). It stays a tripwire — never delete or
   weaken it.
5. **Benchmark campaign** — official campaign on the isolated **m5-atmbench** account
   (never rand-m5) against the standing `baselines.json` floors. Results committed to
   `site/reports/` per the ledger policy.

## Acceptance criteria

- Finding #1 absent from the sc-boundary full JSON payload by identity; **no allow
  attribute used**; no new findings; AU.1/AU.2-owned findings untouched.
- `ack` and `send` have no remaining direct dependency on each other (grep-verifiable;
  the updated tripwire test enforces it).
- New sync+async admission unit tests pass before AND after the move; full `just test`,
  `just lint` green.
- Diff review confirms mechanical relocation: no signature or body changes to moved
  items beyond path adjustments.
- m5-atmbench campaign meets all baselines.json floors (hard merge gate for this PR;
  standing no-regression rule applies).

## Validation

`just test`; `just lint`; sc-boundary full-payload diff (exactly #1 removed);
tripwire test green; benchmark campaign report linked in the PR.
