# Sprint AQ6 — Phase Validation Evidence

Status: draft · Branch: `feature/aq-6-validation-evidence` off
`integrate/phase-aq` · PR target: `integrate/phase-aq`
recommended_agent: Cipher-311d · recommended_model: fast

Closes the phase against the PRD success statement with evidence, mirroring
the AN8/AN12 pattern.

## Deliverables

1. `docs/plans/phase-aq/validation-evidence.md` recording, per requirement
   R1–R8 + R13 (PRD §5), the test/artifact that closes it, with links.
2. Live scenario evidence: US-2 executed end-to-end cross-host ("send missing
   doc to a running agent") — command transcript, delivery proof, and the
   post-sweep state a TTL later.
3. Residue check: after the scenario + sweep, `<known-temp>/atm/` contains
   no expired msg dirs (PRD §8 "nothing leaks").
4. Open-item register: anything discovered but deferred (team addressing,
   Share Extension, Phase 2) listed with a home.

## Acceptance criteria

1. Every Must requirement maps to at least one passing gate; any gap is a
   Blocking finding, not a footnote.
2. Evidence file reviewed by req-qa directly from this doc (no side
   channels).
3. `integrate/phase-aq` → `develop` merge PR opened with the evidence linked.

## Required validation

- Full `just test` + integration suites, macOS + Windows lanes, on the final
  integrate head.

## Non-closure / out of scope

- PRD Phase 2 planning (separate plan once Wyvern chat integration exists).

## Dependencies

- must_follow: AQ1–AQ5 all merged to `integrate/phase-aq`.
- parallel_safe: none.
