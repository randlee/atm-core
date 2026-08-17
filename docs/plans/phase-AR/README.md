# Phase AR — codex-atm host integration (planning)

Phase AR plans atm-core's side of embedding ATM into the codex CLI
(`randlee/codex-atm`, fork of openai/codex): the second production host after
hermes-agent, and the first pure-Rust, crates.io-consuming one.

Status: PLANNING — requirements drafted, pending arch-ctm review. No sprints
cut yet.

Planning source of truth:

- [`plan-phase-AR.md`](./plan-phase-AR.md) — requirements R1–R7 + the R4
  protocol-sequencing decision.

Related:

- Issues #899 (graft connection inversion / daemon long-poll sessions) and
  #900 (fail-loud activation; drop the `.atm.toml` gate) — both graduate to
  prerequisites/decision inputs here.
- ADR-033…ADR-037 (Phase AI): HTTP over UDS locally — the protocol direction
  R4 sequences against.
- Host-side precedent: hermes-agent-atm integration + maintenance pipeline
  (hendrix repo, `hermes-ops/README.md` — START HERE) — the workflow Phase AR
  consumers replicate for codex.
