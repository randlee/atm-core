---
name: measured-behaviour
version: 0.6.1
description: What Jev actually did on a real plan - request sizes it accepts, where in a long bead it stops reading reliably, how the default questions perform on real anchors, run-to-run noise, and the threshold - with the skill rules each number justifies.
---

# Measured behaviour

Every numbered rule in this skill rests on a measurement recorded here: 245 live requests against `jev-1.13.0` on 2026-10-10, on a real plan (six anchors, ten sprint beads of 20k to 99k characters; per-cell tables in `docs/experiments/` of the plan-anchor repo). Re-run the protocol when the model version changes.

## Size

| Measurement | Value | Rule it justifies |
| --- | --- | --- |
| Largest request accepted, one attempt, no 422 | 107,582 bytes, 31,500 input tokens (the largest bead, full text) | The caps are the skill's choice, not Jev's limit |
| Tokens per character of plan text | about 0.31 | A 24,000-byte request is about 7,500 tokens |
| Halve-and-retry fired | 0 of 245 requests | The development gate cap can be 64,000 bytes |
| Verdict changes between full text, description + acceptance, description only, and code reduced to signatures | 0 of 6 pairs; p(addressed) moved at most 0.20, inside noise | The planning gate judges description + acceptance criteria; design text adds tokens, not verdicts |
| Planted contradiction, p(contradicted) by position in one long bead | start of description 0.68 (full) / 0.79 (description + acceptance); 28k deep in design 0.46; end of acceptance 0.40; the sentence alone 0.96; unplanted baseline 0.29 | Jev reads the start reliably, the middle of a long bead poorly. What a bead says about an anchor goes first. The planning-gate cap of 24,000 bytes is "what Jev reads reliably" |
| Code reduced to signatures (`--max-code-lines`), six full-text pairs | changed no verdict | A size tool, not an accuracy lever |
| Always reducing code longer than 8 lines versus sending it whole where the bead fit (13 plan beads by 6 anchors; 11 of 13 fit whole) | underspecified cells 15 → 8; beads passing 1 → 6; a "code for each deliverable" anchor cleared on 5 beads where its `addressed` confidence had been 0.28 to 0.59 when reduced | Code is reduced only when a bead would not otherwise fit, and then as little as possible (`plan-size-and-shape.md`, Long code) |

## Default questions on real anchors

Ten beads by six anchors, generic question sets, description + acceptance criteria, threshold 0.6:

| Measurement | Value | Rule it justifies |
| --- | --- | --- |
| Cells decided at 0.6 | 13 of 60 (6 satisfied, 2 not_addressed, 0 contradicted, 5 not_applicable); 47 `underspecified` | Untuned defaults do not decide; every anchor is tuned before its first gate run |
| Cells decided at 0.5 / 0.4 (offline recompute) | 28 / 43 of 60 | After tuning, 0.5 is a defensible plan-gate threshold; report the leaning of undecided cells |
| Human grading of the 13 decided cells | 4 correct, 7 false passes, 2 false negatives | A generic `satisfied` is not evidence; three false passes came from a homonym ("layer" as stack position) |
| Limiting question among the 47 undecided | `relevant` 23, `addressed` 19, both 5, `contradicted` 0; relevance also produced 4 false `not_applicable` | Relevance wording matters as much as compliance wording; tune both |
| Prohibition anchor ("no document changes a layer's responsibilities ... unless the user approved") | `addressed` no on 10/10 beads, 8 of which comply by silence; not contradicted even by the contradicting sentence alone (p 0.23) | Prohibitions are gated on `contradicted` only (kind `prohibition`); conditional prohibitions are split into a prohibition and an approval record |
| Structural anchor ("every sprint design gives the code for each deliverable") | `addressed` no on 9/10 full-text beads containing 6 to 33 code blocks | Structural anchors are verified by their deterministic check, not by Jev (kind `structural`) |
| Relevance confidence by anchor form | concrete positive requirements 0.85 to 0.89; verbatim shorthand 0.52; conditional prohibition 0.43 | Statements carry concrete, matchable tokens: crate names, paths, commands, issue numbers |
| Scope sentence used by the relevance question | no measurable effect; abstract scope text ignored | `scope` lists bead-level signals ("adds a CLI command", "edits Cargo.toml"), not a topic sentence |

## One tuning round

| Measurement | Value | Rule it justifies |
| --- | --- | --- |
| The layering anchor's `addressed` override (names the four layers, calls out the stacked-PR homonym, lists vocabulary) | `addressed` moved to no at 0.97 to 1.00 on all ten beads; 8 verdicts flipped to `not_addressed`, all correct, including the three false passes; `contradicted` untouched | One concrete override fixes a whole column; generate it at lock time |
| Verdict changes on the five untouched anchors in the same re-run | 8 of 50 cells, identical input; relevance confidence moved more than 0.10 in 15 of 50 (max 0.21) | Cells within 0.10 of the threshold are re-asked once and must agree (`--confirm-near-threshold`) |
| `diff` on the override run | labelled all 8 correct flips as regressions | `diff` reports movement; `--expected` says which direction is correct |

## Two findings the gate missed

Beads adding work outside the approved issues ("also closes <unapproved issue>", "fixes an existing exit-2 defect") and moving consumer test orchestration into the CLI crate scored `contradicted` at 0.17 to 0.19 with the generic question. Scope anchors need a `contradicted` override naming the patterns of unapproved addition ("also closes", "while here", "existing defect"); the deterministic check catches the crate move. This is the planned second tuning round.

## What did not matter

- Which phase label the bead text carried: no effect.
- Code-to-signature reduction on six large full-text pairs: no verdict change. On beads that fit whole it did matter (Size table), so the gate no longer reduces them.
- Model version drift within the day: none; noise was run-to-run, not version-to-version.
