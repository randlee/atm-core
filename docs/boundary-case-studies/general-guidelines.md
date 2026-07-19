# Boundary Leak Review Guidelines

Precise, checklist-form criteria for distinguishing a genuine trait/module
boundary leak from a legitimate cross-boundary need, and for reviewing
trait-surface and pub-item design. This document is the basis for the
ruthless-boundary-qa review checklist. It is deliberately non-narrative.
Worked examples may exist elsewhere, but this checklist is complete without
them.

**Evidence legend** (used across this doc set): **verified** = directly
re-read from commit/blob content in this review pass; **triage-sourced** =
quoted from TTL occurrence entries without independent re-read;
**approximate** = inferred from commit diff/history rather than an exact
citation.

## 1. Boundary leak vs. legitimate cross-boundary need: the test

A dependency, import, or piece of logic crossing a module/crate boundary is
**legitimate** only if ALL of the following hold:

1. **Single decision, single owner.** The fact/decision being used
   (e.g., "is this local or remote," "what backend engine is this") is
   computed in exactly one place, and every consumer receives the *result*
   of that computation, not the raw inputs needed to recompute it.
2. **Opaque surface.** The consumer receives a type/value that hides the
   concrete mechanism (an ID, a resolved enum variant, an opaque handle) —
   not the concrete external-crate type, connection object, or internal
   representation that the owning module uses internally.
3. **Contract lives in the right crate.** If module A implements a trait
   that requires depending on module B, the trait's home crate must be one
   A already legitimately depends on (or a neutral crate below both) — never
   a crate that conceptually sits *above* or *depends on* A.
4. **The dependency direction matches the architecture's intended layering.**
   Storage backends depend on neutral storage contracts, not on the facade
   that composes them. Transport depends on resolved addressing facts, not
   on re-deriving them. A daemon/host depends on a narrow port, not on a
   client-owned subsystem's internal state machine.

It is a **boundary leak** if ANY of the following hold:

1. The same classification/decision logic is implemented independently more
   than once (even if the two implementations currently agree).
2. A concrete type owned by one module (an external-crate type, an internal
   struct, a connection/handle) appears in a signature, return type, or
   public field outside that module.
3. A trait or contract type lives in a crate that only ended up owning it by
   accident of who implemented it first, forcing an otherwise-unnecessary
   dependency edge for any other implementer.
4. A component reaches around an existing abstraction it has access to
   (calls a global side-channel instead of an injected port; re-derives a
   fact instead of consuming the already-resolved one) even though the
   proper channel was reachable.
5. Two conceptually distinct call sites (e.g., "interactive" vs. "replay",
   "local" vs. "remote") are represented as two parallel top-level
   variants/pipelines instead of one pipeline with an internal mode — i.e.,
   a message-semantics distinction has leaked into the transport/dispatch
   shape.

**Concrete test to run during review**: for any cross-boundary fact, grep for
all the places that compute or check it. If more than one call site
independently re-derives the same fact from raw inputs rather than calling a
single shared function/type, that is a leak — regardless of whether the
outputs currently agree.

## 2. Trait-surface narrowing

- Prefer a small number of narrow trait methods over one broad method that
  exposes a concrete backend's full API shape. A trait method's parameter
  and return types should be expressible without importing the concrete
  implementation's types.
- If a trait method's signature would be identical regardless of which
  concrete implementation backs it, that's a sign the trait is doing its
  job. If the signature had to grow a parameter or return type specific to
  one implementation's needs, that implementation detail has leaked into the
  trait.
- Sealed traits (a `Sealed`-supertrait pattern restricting who may
  implement) are the strongest tool for keeping a trait's home crate
  authoritative over its surface — prefer sealing any trait meant to be
  implemented only by the crate's own approved backends/adapters, so
  external crates cannot widen the contract by implementing it themselves.
- When a trait needs to move to a different crate (because its "natural"
  home turned out to be the wrong layer), keep a compatibility re-export in
  the old crate until every consumer has migrated — don't force a
  flag-day cutover that breaks callers mid-move.

## 3. Pub-item minimization

- Default every new item to private or `pub(crate)`. Only widen to `pub`
  when a caller **outside the defining module** genuinely needs it — and
  when that need is confirmed, not speculative ("might be useful later" is
  not sufficient justification).
- A `pub fn`, `pub struct`, or `pub enum` whose only callers are within the
  same module (or the same crate, for `pub(crate)` candidates) should be
  narrowed. Widening visibility is cheap to do later when a real caller
  appears; narrowing later requires an audit of everyone who might have
  started depending on the wider surface.
- Test helpers are not exempt: a `pub`/`pub(crate)` helper introduced only
  to make one test pass, that takes a concrete backend type as a parameter,
  is exactly as much of a leak as production code doing the same thing —
  arguably worse, because it normalizes the pattern for the next person who
  copies the test.
- Re-exports at a crate's root (`pub use other_crate::Thing`) count as part
  of that crate's public surface for this rule. Every re-export needs the
  same "does an external caller genuinely need this" justification as a
  freshly-defined `pub` item.

## 4. Re-export boundary tightening

- Do not re-export a dependency's types through your own module's public
  surface unless that dependency **is** your module's contract — i.e.,
  unless your module's entire purpose is to be a thin, documented wrapper
  around that dependency's API (in which case the re-export is the point).
- If your module re-exports a dependency's type "for convenience" but your
  module's actual purpose is to abstract that dependency away from callers,
  the re-export defeats the abstraction: callers can now depend on the
  concrete dependency's type directly, and swapping the dependency becomes
  a breaking change for them even though your module's own trait/API didn't
  change.
- Rule of thumb: if replacing the underlying dependency (a different SQL
  engine, a different transport library, a different serialization crate)
  would force every re-export consumer to change their imports, the
  re-export was a leak. If it wouldn't, the re-export was fine.
- Watch especially for re-exports that exist only because an error type,
  config struct, or trait from the dependency was convenient to reuse
  as-is. Wrap it in a local newtype or local trait instead unless the
  dependency's type is genuinely meant to be part of your module's stable
  contract.

## 5. Smell list — concrete, grep-able signals

- A concrete external-crate type (e.g. a specific database driver's
  connection/row type, a specific HTTP client's request/response type)
  appearing in a function signature, return type, struct field, or trait
  method **outside** the module that owns the dependency on that crate.
- The same enum discriminant/variant matched or `if let`-checked at more
  than one unrelated call site (i.e., not through a shared dispatch
  function) — especially when the arms do materially different things,
  suggesting each site independently decided how to interpret the variant
  rather than delegating to one owner.
- Two functions with different names in different modules that both take
  "raw" inputs (a string, a socket address, a request struct) and both
  independently decide the same classification (is this remote, is this
  the same team, is this expired) rather than one calling the other or both
  calling a shared third function.
- A `Cargo.toml` dependency edge that points "the wrong way" relative to the
  project's declared layering (a backend/leaf crate depending on a
  facade/composition crate that is supposed to depend on it, not the
  reverse).
- A trait implemented by exactly one crate, where that crate had to add a
  dependency on the trait's home crate purely to reach the trait
  definition, and no other implementer of that trait exists or is planned.
- A component with a legitimate observability/logging/notification port
  already available to a sibling component in the same composition, that
  instead calls a global/ambient channel (a bare `tracing::*!` call, a
  process-wide singleton, a raw file write) rather than the injected port.
- A boundary/contract declaration file (TOML record, ADR, `boundaries.md`
  entry) whose declared owner, state, or allowed-dependents no longer
  matches what the code actually does — a stale declaration is itself a
  leak, because it means the mechanical enforcement checking that
  declaration is no longer checking the real boundary.
- (review heuristic, not grep-first) A "thin wrapper" crate whose public
  surface has grown large enough that most of its dependency's original API
  shape is now reachable through it — the wrapper has stopped being a
  boundary and become a pass-through. Judging "large enough" requires
  reading the wrapper's surface against its dependency's surface; it is not
  a single grep pattern like the other items in this list.
