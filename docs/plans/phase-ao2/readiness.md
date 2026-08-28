---
title: Phase AO2 readiness
phase: AO2
status: blocked
integration_branch: integrate/phase-ao2
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-ao2
---

# Phase AO2 Readiness

Phase AO2 is implementation-complete but not release-closed. This record is
the phase closeout matrix: it distinguishes durable code/report evidence from
physical-host evidence that remains required. No row may be inferred from a
successful local unit test, a rendered report alone, or raw TCP reachability.

## Sprint line

| Sprint | Closure contribution | State |
| --- | --- | --- |
| `AO2.5` / `AO2.5.4` | dedicated benchmark-account isolation and verified snapshot/restore | integrated |
| `AO2.5.3b` | typed, exact-restoring daemon-switch temporary launch overlay | integrated |
| `AO2.6` | bounded admission-writer transaction coalescing | integrated |
| `AO2.7` | required sqlite/UDS/TCP/TCP+TLS matrix contract | integrated |
| `AO2.8` | Windows parity design | **descoped; no coverage claim** |
| `AO2.10`–`AO2.13` | JSON contract, renderer, history, and canonical operator workflow | integrated |
| `AO2.14` | daemon-owned peer connection pooling | integrated |
| `AO2.15` | unattended official benchmark trigger and cleanup contract | integrated |

## Proof matrix

| Proof | Required result | Durable evidence | Gate state |
| --- | --- | --- | --- |
| Runtime boundary | All benchmark, CLI, graft, and cross-host daemon traffic uses the Tokio/Axum `atm-http-runtime`; no legacy synchronous daemon is reintroduced. | architecture guards and `just lint` at the accepted tip | required |
| Benchmark-account safety | The runner refuses an interactive account, stops only an `atmbench`-owned daemon, snapshots/restores only that account, and proves byte-exact restore before/after each measured target. | `scripts/smoke/benchmark_account.py`, snapshot evidence in each campaign | required |
| Four-target macOS campaign | One dedicated M5 account campaign contains `sqlite`, `uds`, `tcp`, and `tcp-tls`, each with raw JSON, accepted durable writes, p50 status, and restore proof. | `site/reports/send-message-benchmark/<campaign>.campaign.json` and the four per-target JSON files | required |
| Report reproducibility | Report panels, phase page, and index are derived from committed JSON and baseline data; no renderer changes a measured value. | `baselines.json`, `historical-record.json`, templates, `phase-ao2.html`, and `just reports-index --check` | required |
| Official trigger | `just benchmark-official` performs the approved account preflight, run, publication, and before/after cleanup without leaving a daemon or temporary benchmark database owned by `atmbench`. | AO2.15 tests and its published operator evidence | required |
| Regression suite | `just lint`, `just test`, and `just validate` pass at the same accepted integration revision. | CI plus accepted-tip command transcript | required |
| Windows physical evidence | Native Windows TCP/TLS evidence is separately recorded and evaluated against its approved baseline. | a future committed Windows campaign | **deferred: `ATM-QA2-004`** |

## Accepted-line evidence rule

An accepted AO2 line must name one immutable `integrate/phase-ao2` revision
and include all of the following at that exact revision:

1. the successful `just lint`, `just test`, and `just validate` transcripts;
2. a complete dedicated-`atmbench` M5 four-target campaign with raw JSON,
   rendered panel, p50/pass-fail status, and byte-exact restore evidence for
   every target;
3. `just reports-index --check` against the committed report set; and
4. an explicit disposition for the deferred Windows row.

The Windows row is not satisfied by AO2.8: that sprint is descoped and
`ATM-QA2-004` remains deferred. Until a separately accepted native Windows
campaign supplies its evidence, this file remains `status: blocked` and
Phase AO2 must not claim cross-platform physical-benchmark closure.

## Operator source of truth

The executable procedure is
[`benchmark-run`](../../../.claude/skills/benchmark-run/SKILL.md). It is the
only authority for running, reviewing, publishing, and retaining a benchmark
attempt. This readiness document records phase gates; it does not duplicate
or replace that procedure.
