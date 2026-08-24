# Sprint AQ2 — CLI Surface and Same-Host Delivery

Status: draft · Branch: `feature/aq-2-cli-surface` off `integrate/phase-aq` ·
PR target: `integrate/phase-aq`
recommended_agent: arch-ctm · recommended_model: deep-reasoning

Makes the pipeline real on one host:
`atm teams --json | <picker> | atm send --attach "$@" --from-json`.

## Deliverables

1. **Picker projection audit of `atm teams --json`**: verify member entries
   carry `{id, host, cwd, status}`. Any missing field becomes a deliverable
   in this sprint (registration or projection change), recorded in the PR —
   not silently absent.
2. **`atm send --attach <path>...`** (repeatable): computes sha256 + size,
   copies same-host content into `attachment_dir()` (AQ1), populates
   `attachments` on the envelope. Missing/unreadable path → hard error before
   any send.
3. **`atm send --from-json`**: reads `{recipients: [...], note?: string}`
   from stdin; fan-out is one envelope per recipient; `note` becomes message
   text with `note_source` per AQ1. Mutually exclusive with positional `to`
   and `--stdin`; conflicts rejected by clap, not runtime checks.
4. **Cancel semantics (R5/R13)**: empty stdin or malformed JSON → nonzero
   exit, zero sends, zero files created. Attachment staging happens only
   after recipient JSON validates.
5. **Cross-host refusal stub**: recipient on another host → explicit
   "cross-host attachments land in AQ3" error, never a reference-only send.

## Acceptance criteria

1. Truth-table tests for `--from-json`: valid multi-recipient → N envelopes
   each with identical `attachments` refs; empty/malformed/cancel → exit ≠ 0
   and `attachment_dir()` for the would-be msg-ids absent.
2. Same-host end-to-end test: file lands under `<known-temp>/atm/<msg-id>/`,
   sha256 verified, recipient reads envelope with populated `local_path`.
3. Duplicate content to two recipients produces two envelopes whose
   attachments share `sha256` (dedupe observable at the reference level).
4. `atm teams --json` output validates against the picker input schema in the
   PRD (§4.2) via a fixture test.
5. `just test` both CI lanes; no clippy warnings in touched crates.

## Required validation

- `just test` workspace, macOS + Windows CI lanes.
- One recorded same-host demo transcript (command + resulting envelope JSON)
  committed as evidence on the sprint branch.

## Non-closure / out of scope

- Cross-host fetch (AQ3). Sweeper (AQ4). Any UI (AQ5).

## Dependencies

- must_follow: AQ1 (contract) — merge-forward before every dev/fix round.
- Dispatch precondition: `integrate/phase-aq` created from `develop`.
- parallel_safe: none at start; AQ3/AQ4/AQ5 fan out after this merges.
