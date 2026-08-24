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

The evidence file is a versioned, reviewable table with one row per Phase-1
requirement:

```text
requirement | deliverable/sprint | command or live artifact | exact SHA | result | reviewer
R1..R8,R13 | AQ1..AQ5           | path + command             | 40-hex     | PASS/OPEN | name
```

R9–R12 are Phase-2 requirements and must appear in the open-item register as
explicit non-closure, not as silently missing rows. Every `PASS` row links to
the raw transcript, test output, or committed report that a reviewer can open
directly from this sprint branch.

## Acceptance criteria

1. Every Must requirement maps to at least one passing gate; any gap is a
   Blocking finding, not a footnote.
2. Evidence file reviewed by req-qa directly from this doc (no side
   channels).
3. `integrate/phase-aq` → `develop` merge PR opened with the evidence linked.

## Paths to delete

None. AQ6 adds evidence and an open-item register; it must not delete raw
artifacts, sprint reports, or temporary directories as a substitute for the
AQ4 residue proof.

## Required validation

- Full `just test` + integration suites, ubuntu + macOS + Windows lanes, on
  the final integrate head.
- The two-daemon peer-pair suite, shell-script harness, and residue check named
  by AQ2–AQ5 must be rerun at the exact final `integrate/phase-aq` SHA.

## Non-closure / out of scope

- PRD Phase 2 planning (separate plan once Wyvern chat integration exists).

## Dependencies

- must_follow: AQ1–AQ5 all merged to `integrate/phase-aq`; merge-forward is
  required before every evidence fix round so the matrix describes the actual
  integrated head.
- parallel_safe: none — AQ6 consumes every public contract and owns the only
  phase-close evidence artifact.
