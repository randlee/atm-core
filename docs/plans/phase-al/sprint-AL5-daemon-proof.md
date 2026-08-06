# AL.5 — Thin Daemon Composition and Cross-Host Proof

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.3 and AL.4.
**unblocks:** AM.2, AM.3, and AM.4.
**parallel_safe:** AM.1 inventory only.

## Deliverables

1. Make `atm-daemon` a composition/lifecycle root for `atm-http-runtime`:
   construct sealed core adapters, select listener configuration, inject the
   received-hook implementation, start, and gracefully stop.
2. Record an explicit legacy removal ledger for AM from actual remaining
   references.
3. Produce reproducible local and M5 cross-host evidence using the new runtime.
4. Measure benchmark parity/regression against the recorded pre-migration
   baseline; report the measured numbers and environment.

## Acceptance criteria

- The daemon has no concrete SQLite dependency, tmux/graft reference, peer
  application decoder, manual HTTP framing, or resend startup.
- Local CLI, localhost/self target, and M5 send each reach the new canonical
  route and receive typed normal results.
- The proof records one nudge for a new receipt, none for idempotent duplicate,
  and a warning-only hook failure.
- Benchmark result is recorded; any regression is investigated before AM
  deletion begins.

## Required validation

- `just test`, formatter, and lint suite
- local smoke through the active AL daemon
- M5 clean-checkout cross-host smoke
- benchmark command and checked-in/attached result artifact
- review of every row in the shared boundary checklist

## Non-closure

AL.5 does not delete legacy files. It is the proof gate authorizing AM to do
so.
