---
status: in-progress
branch: evidence/bc-7-release-validation
pr_target: develop
owner: arch-ctm
---

# sprint bc.7 — release validation on develop (benchmark + colima)

Accountable owner: `arch-ctm@atm-dev`. Lead: `fenix@atm-dev`.

Rand (2026-09-23 UTC): phase-bc is on `develop` (#1576 `c66587456`, ledger
closure #1580 `982c1f20a`, post-merge CI 19/19). Before the release from
`main`: tag `prerelease/v1.6.1` on develop (done by fenix), run the official
benchmark campaign from the dedicated `atmbench` OS account, and run the
colima integration sequence against that prerelease. Evidence only; no code.

## deliverables

1. Official benchmark campaign per `.claude/skills/benchmark-run/SKILL.md`
   §8, triggered from the developer account over ssh as `atmbench@rand-m5.local`
   with `just benchmark-official --branch evidence/bc-7-release-validation`;
   the runner commits and pushes the campaign evidence to this branch. Review
   with `just benchmark-show`; record host, source revision, all targets and
   each verdict verbatim in this doc.
2. Colima integration per `scripts/integration/run_colima.py`:
   `just integration colima --testbed ~/github/atm-hermes-testbed` from this
   worktree once `gh release view prerelease/v1.6.1` shows the archive assets.
   Commit `site/reports/integration/colima/<stamp>/` byte-for-byte as emitted;
   record the verdict line verbatim here.
3. One PR `evidence/bc-7-release-validation -> develop` with evidence and this
   doc only.

## rules

- Visibility, not code: no new Python validators, classifiers, diagnostics or
  output filters for agent issues. An agent-side failure is diagnosed by
  reading the transcript/logs in the evidence and, if needed, by nudging the
  agent through `atm send`; never by adding code. A step failing on a fresh
  fixture is a finding, reported verbatim, not patched around.
- Never touch sqlite directly; never run a benchmark from the developer
  account; never restart a daemon that is not `atmbench`'s.
- No version bump, tag, or publish: `prerelease/v1.6.1` is the only tag and
  fenix owns it.

## evidence

(filled by arch-ctm)
