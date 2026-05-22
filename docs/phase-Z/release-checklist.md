# Phase Z Release Checklist

## Purpose

Authoritative final executable validation and release checklist for `Z.4`.

## Record Schema

Each checklist row must record:

- `validation_id`
- `flow_or_gate`
- `expected_result`
- `verdict`
- `evidence`
- `notes`

## Required Gate Coverage

The final release checklist must include:

- final rerun of the approved executable validation set, which consists of:
  - the promoted `Z.1` / `Z.2` smoke coverage represented by
    `docs/phase-Z/smoke-checklist.md`
  - the promoted `Z.3` operator-facing canary coverage represented by
    `docs/phase-Z/canary-dogfood-checklist.md`
- confirmation that all `Z.3` findings are fixed or explicitly dispositioned
- confirmation that the release verdict in `docs/phase-Z/readiness.md`
  references this checklist result
