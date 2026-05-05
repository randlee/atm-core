# ADR-001 — Sealed Trait Pattern for atm-core Boundary Traits

| Field | Value |
|---|---|
| ID | ADR-001 |
| Status | **Accepted** |
| Date | 2026-05-04 |
| Deciders | Rand Lee |
| Relates to | ARCH-001 |
| Supersedes | — |

---

## Context

`atm-core` defines the boundary traits (ports) for the `agent-team-mail` workspace. The
original design used a true Rust sealed pattern:

```rust
// atm-core (original design)
mod sealed {
    pub(super) trait Sealed {}
}

pub trait MessageTransport: sealed::Sealed {
    fn send(&self, msg: Message) -> Result<()>;
}
```

`mod sealed` is private to `atm-core`. Only `atm-core` can implement `sealed::Sealed`,
therefore only `atm-core` can produce types that satisfy `MessageTransport`. This is
compiler-enforced — no external crate can bypass it.

### Phase R Decision

Phase R of the project introduced `atm-daemon` and `atm-rusqlite` as separate adapter
crates. These crates must implement `atm-core` boundary traits directly:

```
atm-core        — defines boundary traits (ports)
atm-daemon      — implements atm-core traits (daemon transport adapter)
atm-rusqlite    — implements atm-core traits (SQLite storage adapter)
atm             — consumes atm-core traits only; never references adapters directly
```

Under this topology, `atm-daemon` and `atm-rusqlite` must be able to call
`impl MessageTransport for DaemonTransport`. The original `mod sealed` (private)
prevents this — the supertrait `sealed::Sealed` is invisible to any crate outside
`atm-core`.

### QA Review (ARCH-001)

arch-qa raised ARCH-001 requesting reversion from `pub mod sealed` back to `mod sealed`.
rust-qa-agent defended `pub mod sealed` as accepted convention.

Both agents were reasoning about the wrong problem. The real issue is that the Phase R
cross-crate adapter topology is structurally incompatible with the original sealed design.
This is an architectural constraint, not a convention debate.

---

## Decision Drivers

- Agents must not be able to implement `atm-core` boundary traits in unauthorized crates
- The compiler should enforce as much as possible; tooling enforces what the compiler cannot
- The adapter topology (`atm-daemon`, `atm-rusqlite` as separate crates) is a Phase R
  requirement and is not negotiable for this phase
- Historical failure mode: agents add unauthorized crate references and widen visibility
  to make code compile, cementing violations into the architecture

---

## Options Considered

### Option 1 — Keep true sealing (`mod sealed` private)

All trait impls must remain in `atm-core`. Adapter crates provide configuration and data
types only; `atm-core` owns all impl blocks.

**Pros:** Compiler-enforced. Strongest possible boundary. No convention required.

**Cons:** Contradicts the Phase R cross-crate adapter plan. `atm-core` becomes a
bottleneck — every new adapter variant requires changes to `atm-core` itself. Couples
the port definition crate to all adapter implementations, which inverts the intended
dependency direction.

**Verdict:** Rejected for Phase R. Valid target for a future redesign if the adapter
model changes such that impl co-location becomes viable.

### Option 2 — Accept `pub mod sealed` (workspace-convention seal)

Make `mod sealed` public. Adapter crates can see and implement `sealed::Sealed`. The
sealed pattern becomes a documented convention rather than a compiler-enforced boundary.
Unauthorized impls are caught by lint tooling (`just lint` boundary checks) and forbidden
dependency edge rules, not by `rustc`.

**Pros:** Compatible with Phase R adapter topology. Adapter crates compile without
changes. Familiar pattern — used in tokio, axum, and other production crates.

**Cons:** Sealing is no longer compiler-enforced. An agent or developer in an external
crate could implement `sealed::Sealed` if they take the dependency. Requires lint gates
to carry the enforcement weight.

**Verdict:** Accepted for Phase R with mitigations documented below.

### Option 3 — Redesign boundary ownership

Remove sealing entirely, or restructure the crate model (e.g., introduce an
`atm-core-internal` crate that is `publish = false` and holds the sealed marker,
accessible to workspace members but not to downstream crates.io consumers).

**Pros:** Could recover stronger enforcement without blocking adapter crates.

**Cons:** Larger refactor. Adds crate complexity. `publish = false` enforcement only
matters if `atm` crates are published to crates.io — not a current requirement.
Premature for Phase R scope.

**Verdict:** Deferred. Revisit if `atm` crates are published to crates.io or if the
adapter model changes significantly.

---

## Decision

**Accept Option 2 — `pub mod sealed` as the Phase R cross-crate pattern.**

This is explicitly a **workspace-convention seal**, not a true Rust seal. The language
boundary is replaced by three enforcement layers:

### Discoverability Mitigation — `#[doc(hidden)]`

```rust
// atm-core
#[doc(hidden)]
pub mod sealed {
    pub trait Sealed {}
}
```

`#[doc(hidden)]` hides `sealed::Sealed` from generated documentation. External consumers
will not encounter it through normal API discovery. This is the standard production
approach (tokio, axum). **This does not enforce the boundary** — it only reduces
accidental discovery. A crate that explicitly depends on `atm-core` can still reference
`atm_core::boundary::sealed::Sealed` regardless of `#[doc(hidden)]`.

### Enforcement Layer 1 — Boundary lint (`lint_boundaries.py` + manifest checks)

`lint_boundaries.py` and `lint_manifests.py` gate CI on every push. `lint_boundaries.py`
checks that trait impl sites match the permitted records in `docs/*/boundaries.md`.
`lint_manifests.py` verifies workspace `Cargo.toml` dependency declarations against
allowed edges.

Boundary records in `docs/atm-core/boundaries.md`, `docs/atm-daemon/boundaries.md`, and
`docs/atm-rusqlite/boundaries.md` document the permitted impl sites. Any impl in a crate
not listed in the corresponding record surfaces as a lint finding and fails CI.

### Enforcement Layer 2 — Forbidden dependency edges

`atm` must never depend on `atm-daemon` or `atm-rusqlite`. `atm-core` must never depend
on any adapter crate. These rules are enforced by `lint_boundaries.py` and
`lint_manifests.py` — not by code review as a primary layer.

```
atm-core        ← atm-daemon      (atm-daemon depends on atm-core, not vice versa)
atm-core        ← atm-rusqlite    (same direction)
atm             ← atm-core        (atm depends on atm-core only)
atm             ✗ atm-daemon      (FORBIDDEN)
atm             ✗ atm-rusqlite    (FORBIDDEN)
atm-core        ✗ atm-daemon      (FORBIDDEN)
atm-core        ✗ atm-rusqlite    (FORBIDDEN)
atm-daemon      ✗ atm-rusqlite    (FORBIDDEN — trait-only/reference-only)
```

---

## Consequences

### Positive

- Phase R adapter crates compile without structural changes
- Unauthorized impls caught at lint time (`just lint`, pre-commit, CI)
- Forbidden dependency edges caught at lint time
- Pattern is consistent with production Rust ecosystem conventions

### Negative

- Sealing is no longer compiler-enforced at the language level
- A sufficiently determined agent (or developer) in an external crate *can* implement
  `sealed::Sealed` if they take the `atm-core` dependency — this is only detectable via
  lint, not rustc
- Lint gates must run reliably for enforcement to hold — gaps in lint coverage mean gaps
  in boundary enforcement

### Accepted Risk

The workspace is not currently published to crates.io. The primary threat model is
**agents taking the easy path within the workspace**, not external consumers bypassing
the boundary. Lint gates are sufficient for this threat model in Phase R.

---

## Future Refactor Trigger

If any of the following conditions arise, reopen this ADR and evaluate Option 1 or
Option 3:

- `atm-core` or adapter crates are published to crates.io (external consumer threat
  model becomes real)
- The adapter model changes such that impl co-location in `atm-core` becomes viable
  without coupling the crate to all adapter variants
- A new adapter crate is added that is not first-party — boundary records must remain
  audited and finite

---

## Action Items

| Action | Owner | Gate |
|---|---|---|
| Add `#[doc(hidden)]` to `pub mod sealed` in `atm-core` | arch-ctm | Phase R |
| Add sealed module doc comment (workspace-convention language) | arch-ctm | Done (49861c4) |
| Verify boundary records reference permitted impl sites | arch-ctm | Phase R |
| Add ADR reference to `AGENTS.md` — agents must not modify `pub mod sealed` without ADR review | team-lead | Phase R |
| Close ARCH-001 | team-lead | Done |

---

*ADR-001 | agent-team-mail | 2026-05-04*
