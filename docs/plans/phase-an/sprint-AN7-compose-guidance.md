---
title: AN.7 Compose Passthrough, Guidance, Migration Telemetry
status: draft
branch: feature/pan-s7-compose-guidance
worktree: ../atm-core-worktrees/feature/pan-s7-compose-guidance
target: integrate/phase-an
---

# AN.7 — Compose Passthrough, Guidance, Migration Telemetry

**recommended_agent:** Cipher-311d/fast (bounded CLI plumbing and
documentation).
**must_follow:** AN.3; merge AN.3's pushed integration line before each dev or
fix round (shares the send-admission module and templated-send flags the
guidance documents).
**unblocks:** AN.8.
**parallel_safe:** AN.4, AN.5, AN.6 (non-intersecting modules: this sprint
owns the compose command, the send-admission path detector, and docs).

**traceability:** plan-phase-an.md Decision 6; Send/read flow (`atm
compose`); path-body telemetry paragraph. Requirement IDs assigned during
plan hardening.

## Deliverables

1. `atm compose` passthrough through the core renderer port backed by the
   dedicated `sc-composer` adapter: validate/dry-run/render a
   template + vars to stdout with the same resolution behavior as `atm send
   --template`, no mailbox interaction, output byte-identical to invoking
   sc-compose directly with equivalent inputs.
2. Path-only-body detector at send admission, per the plan's detection rule
   (body < 512 bytes, matches an existing-file/absolute/homedir/worktree
   path shape, no other prose). On match: WARN on the sender's CLI naming
   the templated-send replacement, record `content_format='path-ref'` on the
   stored message, emit a structured observability event. **No rejection.**
3. `docs/team-protocol.md`: a "send content, not paths" section with a
   worked example converting today's render-to-file-and-send-path pattern
   into `atm send <agent> --template <t> --vars <v>`, and a note on
   `atm compose` for previewing.
4. Help-text updates for send/compose covering the new flags and the
   path-body warning.

## Acceptance criteria

- `atm compose` output is byte-identical to direct sc-compose invocation for
  the AN.1 fixture templates (both success renders and validation-failure
  diagnostics/exit codes).
- Path-only fixture bodies trigger the WARN, store `path-ref`, and emit the
  observability event; a fixture corpus of prose *containing* paths produces
  zero false positives.
- The team-protocol section's worked example executes verbatim against a
  fixture team (doc-tested, not prose-only).
- The command adapts the core contract only; it has no direct
  `sc-composer` dependency.

## Required validation

- passthrough byte-equality tests (success and failure paths)
- detector fixture corpus (positive and negative)
- observability event assertion test
- executable-example doc test
- cargo test/format/lint suite

## Non-closure

Hard rejection of path-only bodies is explicitly not closed here — it is a
phase-AO candidate gated on `path-ref` telemetry approaching zero. No search
or read behavior changes land in this sprint.
