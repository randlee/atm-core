# Repository Quality Policy

This file contains repository-specific QA policy. Reusable agents and skills
must read this file rather than embedding repository names, commands,
interfaces, approval authorities, or temporary architectural exceptions in
their own prompts. The manifest `.claude/project/orchestration.yaml` names
this file as `policy`.

## Repository Baseline

- Default comparison branch: `develop`
- Requirements index: `docs/requirements.md` (REQ ids inline, e.g.
  `REQ-ATM-CMD-001`, `REQ-CORE-BOUNDARY-001`)
- Architecture index: `docs/architecture.md`; ADRs are
  `docs/adr/ADR-NNN-<slug>.md`, indexed by `docs/adr/INDEX.md` (the
  adr-index lint)
- Project plan: `docs/project-plan.md`
- Team protocol: `docs/team-protocol.md`
- Developer roster: `.atm.toml` and the ATM roster (`atm members`) are
  authoritative for live identities; there is no roster file.

## Reviewer Policy

- Initial implementation review (sprint QA-1): `req-qa`, `arch-qa`,
  `ruthless-boundary-qa`, `rust-qa-agent`, `rust-best-practices-agent`, and
  `rust-service-hardening-agent`
- Fix verification (sprint QA-2 and later): `req-qa`, `arch-qa`, and
  `rust-qa-agent` only. `ruthless-boundary-qa`,
  `rust-best-practices-agent`, and `rust-service-hardening-agent` run in
  round 1 only and are NEVER re-run on a fix-verification round (Rand's
  standing rule; `quality-mgr.md`, "Boundary-review deployment rule")
- Plan review QA-1: `plan-scope-reviewer`, `req-qa`, `arch-qa`,
  `ruthless-boundary-qa`, `rust-best-practices-agent`,
  `rust-service-hardening-agent`, and `ceremony-qa`. `plan-scope-reviewer`
  judges the plan's shape against the architecture's boundary map: the wave
  table, the critical path and width it recomputes from the bead graph,
  `owned_paths` disjointness within a wave, and the contract artifact named
  by every `must_follow` edge. Its assignment is rendered with
  `plan-scope-reviewer-assignment.json.j2` (codex-orchestration for a plan in
  markdown, atm-bd-orchestration for a plan in beads) over the same plan
  files as `req-qa` and `arch-qa`, plus the phase plan or root
- Plan review QA-2 and later: `req-qa` and `arch-qa` scoped to the
  dispatched findings, and `plan-scope-reviewer` in full again every round:
  its checks are recomputed from the graph, not carried, so a fix round that
  lengthens the critical path, adds an ordering rule or moves a shared file
  into a layer sprint fails the round even when every carried finding is
  fixed. Re-dispatch `ruthless-boundary-qa`, `rust-best-practices-agent`,
  `rust-service-hardening-agent`, or `ceremony-qa` only to verify its own
  QA-1 findings, verification-locked to those ids. Plan QA is capped at 3
  rounds (`plan_qa_cycle_limit`); a round that leaves only minor findings
  reports `PASS — minor fixes required, no re-QA`
- Plan review rulings: the remedy for shared types, shared files or a shared
  version baseline is to hoist the artifact into the contract or integration
  sprint (plan guidelines, "Ownership And Dependency Relations"), never an
  edge. `quality-mgr` lists every finding whose remedy would add a
  `must_follow` edge, an ordering rule or a merge-order clause as a proposed
  `hoist` ruling for the lead (`quality-mgr.md`, "Hoist Rulings"), in the
  same way as its proposed `rejected: ceremony` rulings. The lead accepts an
  edge only with a recorded reason naming the artifact that could not be
  hoisted, and recomputes the wave table's critical path, width and sprint
  count after each round of rulings. An edge that lengthens the critical
  path is the user's ruling, not the lead's: the lead puts the proposed edge,
  the artifact and the new critical path to the user and records the answer
  in the ruling; without that record the round is redone. The baseline
  critical path is the architecture's layer count (contract, the layers
  `docs/architecture.md` defines, integration), which differs per repository;
  a plan is not held to a fixed number of waves, but every wave past that
  baseline carries a user-approved reason
- `ceremony-finding-screen`: every sprint or plan QA round that has
  findings, over all of them, before the report is posted
- Phase-end review: the initial implementation set plus `flaky-test-qa`
- Run `flaky-test-qa` earlier when tests changed or instability is suspected
- `schema-reviewer`: in plan review and phase-ending review whenever a
  governed interface (below) is in scope
- Every QA round, plan or sprint, requires an open PR; the rendered report
  is posted to it every round

## Validation Policy

- Lint: `just lint` (codespell, line counts, ADR index, site links,
  identity literals)
- Tests: `just test`
- Phase-end validation: `just validate` (runs
  `scripts/validate_release.py`, the release preflight). quality-mgr passes
  it to `rust-qa-agent` as `artifact_commands` on every `phase_end`
  assignment and never runs it in the foreground.

## Plan Naming

Everything in file names, branch names, front matter `id` values and
template variables is lower case. Prose may write "Phase BC" and "BC.4".

| Thing | Form | Example |
|---|---|---|
| Phase id | next unused letter pair, lower case | `bc` |
| Sprint id | `<phase>-<n>`, `n` from 1 | `bc-4` |
| Plan directory | `docs/plans/phase-<phase>/` | `docs/plans/phase-bc/` |
| Phase plan | `docs/plans/phase-<phase>/phase-<phase>-plan.md` | `phase-bc-plan.md` |
| Sprint doc | `docs/plans/phase-<phase>/sprint-<phase>-<n>-<slug>.md` | `sprint-bc-4-task-ledger-writer.md` |
| Plan branch | `plan/phase-<phase>`, PR to `develop` | `plan/phase-bc` |
| Phase branch | `integrate/phase-<phase>`, cut from `develop` | `integrate/phase-bc` |
| Sprint branch | `sprint/<phase>-<n>-<slug>` | `sprint/bc-4-task-ledger-writer` |
| Fix layer | `fix/<phase>-<n>-<slug>`; phase-level: `fix/<phase>-<slug>` | `fix/bc-4-qa1` |

The full rule set is "Naming" in
`.claude/skills/plan-hardening/sprint-planning-guidelines.md`. Stack
mechanics: `docs/development/gh-stack-guidelines.md` and the `gh-stack-view`
skill.

## Governed Interfaces

`docs/adr/ADR-061-governed-interface-schema-versioning.md` governs three
interfaces: the HTTP/peer API, the Herdr IPC, and the SQLite storage
schema. Each is semver versioned; an optional addition is a minor bump; a
breaking change needs Rand's explicit, recorded approval and sign-off.
`schema-reviewer` (`.claude/agents/schema-reviewer.md`) holds the evidence
paths and rules.

## Repository Exceptions

- ADR ids are in file names under `docs/adr/` (`ADR-061-*.md`) and are not
  `## ADR-...` sections of `docs/architecture.md`; REQ ids are inline in
  `docs/requirements.md`. `validate-plan` matches an id against document
  content and file names (manifest `docs.*`).
- Existing sprint docs (phases before beads orchestration) carry no
  `requirements` / `adrs` front matter lists; do not raise missing-list
  findings for them.
