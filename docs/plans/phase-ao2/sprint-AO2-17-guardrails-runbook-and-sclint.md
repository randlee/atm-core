# Sprint AO2.17 — Nightly Benchmark Runbook + sc-lint Extraction Proposal

Status: draft · Branch: `feature/ao2-17-guardrails-runbook-sclint` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: Cipher-311d · recommended_model: fast

Third guardrail sprint (see AO2.15 header for motivation): the ops and
ecosystem halves — regression detection latency drops to "next morning,"
and the proven in-repo lint design is proposed to sc-lint for reuse.

## Deliverables

1. **Nightly hardware-run runbook**
   `docs/benchmark-nightly-runbook.md`: the documented `m5-atmbench`
   invocation (`ATM_CAPACITY_HOST_LABEL=m5-atmbench just benchmark` under
   the isolated benchmark account per ADR-052 — official evidence is
   valid ONLY from manifest-listed isolated accounts, codifying the
   2026-08-24 host-isolation finding), publication through the AO2.10–13
   pipeline, and the escalation path. Actual scheduling (launchd/cron)
   is an ops step the runbook documents; the repo ships no scheduler.
2. **Floor-breach alert hook** in `benchmark_report.py`: when a rendered
   campaign contains any FAIL against `baselines.json`, emit an ATM
   message to team-lead (`atm send team-lead …` with campaign id, target,
   p50 vs floor, contract hash), best-effort (alert failure never fails
   the render; it logs).
3. **Isolated-account allowlist**
   `tools/benchmark-hosts.toml` (revives the AO2.9 idea in minimal form):
   the manifest of host_labels whose evidence counts as official
   (`m5-atmbench`, future windows equivalent):

```toml
schema_version = 1
# host_labels whose evidence is official; changes quality-mgr-gated.
host_labels = ["m5-atmbench"]
```

   `benchmark_report.py` badges non-listed-host evidence "unofficial environment" (rendered,
   retained, never candlestick-plotted against official floors —
   mirroring AO2.16's `"pre-contract"` state).
4. **sc-lint extraction proposal**: file the generalized hot-path-guard
   design (manifest schema + sentinel regions + lexical banned-token
   classes + clippy-deny cross-check + touch-triggers-evidence gate) as
   an issue/PR-skeleton against the sc-lint repo, referencing AO2.15's
   in-repo implementation as the proving ground. Deliverable is the filed
   proposal (URL recorded in this doc's QA history), not the sc-lint
   implementation.

## Acceptance criteria

1. (D1) The runbook exists at the named path, covers invocation,
   account/isolation preconditions, publication, wyvern review step, and
   the alert escalation path; `req-qa` can execute it step-by-step
   without external context.
2. (D2) Fixture test: a synthetic floor-breach campaign fires exactly one
   ATM alert containing campaign id/target/p50/floor/hash; alert-send
   failure is logged and the render still succeeds.
3. (D3) Fixture: evidence with a non-listed host_label renders with the
   "unofficial environment" badge and is absent from official
   candlesticks; listed-host evidence unaffected.
4. (D4) The sc-lint proposal is filed and its URL recorded here; it
   contains the manifest schema, both enforcement classes, and the
   evidence-gate design.
5. All suites green on all three CI lanes.

## Required validation

- One live nightly-runbook execution on m5-atmbench (evidence committed)
  before quality-mgr dispatch.
- quality-mgr sign-off on the initial `benchmark-hosts.toml` allowlist.

## Non-closure / out of scope

- Scheduler provisioning on m5 (ops step, documented only).
- sc-lint implementation.
- Windows isolated benchmark account creation (allowlist gains it when it
  exists).

## Dependencies

- must_follow: AO2.16 (alert hook and badges build on the contract-hash /
  baselines plumbing). Merge-forward trigger: AO2.16 dev push.
- parallel_safe: AO2.14, AO2.15 **for deliverables 1–3** (docs/tools/
  report-layer only; no runtime crates, no manifest files). Deliverable 4
  alone carries a PR-completion trigger on AO2.15 (the proposal FILES only
  once AO2.15's PR completes; its skeleton may draft earlier).
