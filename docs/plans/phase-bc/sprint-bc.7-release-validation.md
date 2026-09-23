---
status: complete
branch: evidence/bc-7-release-validation
worktree: /Users/randlee/github/atm-core-worktrees/evidence/bc-7-release-validation
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

## Evidence

Official campaign `20260923T212808Z-m5-atmbench` ran on host `m5-atmbench`
from source revision `23162f0ef23d8a12bfd0dc408d5374c4a236e809` (the branch
parent). The runner produced and published the immutable campaign artifacts in
`ae2b79207`; the generated report page was refreshed in `6bca4e11f` and the
combined evidence is on this branch at `6bca4e11fd571e95f1c97169f2d73a98561c7498`.
The official verdict lines were:

```text
PASS required f8 target sqlite: p50=47200.67 msg/s accepted=918000/918000
PASS required f8 target uds: p50=19133.51 msg/s accepted=380000/380000
FAIL required f8 target tcp: p50=16587.75 msg/s accepted=330000/330000
PASS required f8 target tcp-tls: p50=14988.44 msg/s accepted=298000/298000
```

The campaign status is a measured `FAIL` because TCP was below its then-current
17000 floor; the result is retained as publishable evidence.

Colima used prerelease `v1.6.1` (ATM 1.6.1) after all five archive assets and
`checksums.txt` were visible. The canonical renderer emitted:

```text
colima integration: FAIL
```

The byte-preserved evidence is under
`site/reports/integration/colima/20260923T215040310333Z/`, with
`integration.json` source revision
`6bca4e11fd571e95f1c97169f2d73a98561c7498` and status `FAIL`. The fixture's
first Hermes-skills step recorded one setup PASS but no provider-backed skill
reports; the emitted run log records that Hermes had no configured AI provider.
The task-start and assignment prompt steps completed PASS before the fixture
was terminated at the harness boundary; no evidence files were edited.
