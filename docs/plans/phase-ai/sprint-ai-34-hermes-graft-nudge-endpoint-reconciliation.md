---
title: AI.34 Hermes graft nudge-endpoint reconciliation
status: in_progress
branch: fix/hermes-nudge-endpoint-mismatch
target: integrate/phase-ai-31-33
depends_on: AI.31, AI.32
requires_merged_pr: PR #678 (RosterHarness::Hermes/PythonGraft, merged to develop @ cf8511ae)
---

# AI.34 — Hermes graft nudge-endpoint reconciliation

## Closure

Live Hermes nudge delivery to a Python-graft roster member (skillrx) succeeds
end-to-end on a release-built ATM daemon: the publisher-written endpoint
record and the daemon's runtime_health resolver agree on a single canonical
path for any roster member whose `workspace_root` diverges from its
`home_dir`.

## Background

Found by quality-mgr's HERMES-SMOKE-QA-1 live-evidence follow-up (Cipher-311d),
on `develop @ cf8511ae` (post-merge of PR #678). Nudge delivery times out:
the Python graft publisher writes its endpoint file under
`{workspace_root}/.atm/graft/{team}/{agent}.json`
(`crates/atm-graft/src/lib.rs:339-343`, via
`graft_receiver_record_path_from_home(options.workspace_root())`), but
`atm-daemon`'s runtime_health resolver looks under the roster member's
metadata `home_dir` instead
(`crates/atm-daemon/src/runtime_health.rs:101-111`, via
`canonical_home_dir(&member.metadata_json)` in
`crates/atm-core/src/schema/agent_member.rs:45-47`).
`workspace_root` and `roster.home_dir` diverge for skillrx, so the daemon
never finds the endpoint file the publisher wrote.

Triage record: `.triage/phase-AI/findings/AI-HERMES-NUDGE-ENDPOINT-MISMATCH.ttl`
(severity: blocking).

## Required fixes

1. Reconcile the publisher-side and daemon-resolver-side root paths so they
   always converge on the same endpoint file location. Per the triage
   record's closure note, pick one of: (a) both sides use `workspace_root`,
   (b) both sides use `home_dir`, or (c) introduce a single canonical
   resolver function both call.
2. Prefer option (c) if it doesn't expand scope unreasonably — a canonical
   resolver structurally prevents this class of divergence from recurring.
3. Add a regression test: publisher writes an endpoint record, daemon
   resolver looks it up, assert they resolve to the identical path for a
   roster member whose `workspace_root` differs from `home_dir`.

## Required validation

- Live nudge smoke against skillrx@hermes succeeds (Cipher-311d's pre-fix
  baseline: message `01KYKGBRMPHDM1WS2Z1R8DYTQT`, AI32 build `746c69ee` —
  constructors/options/activation/snapshot/duplicate-activation/send
  persistence already pass; nudge callback is the only remaining failure).
- `just lint`, `just test` pass.
