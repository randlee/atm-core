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
`checksums.txt` were visible. Verdict on 1.6.1:

```text
colima integration: PASS (all seven skill slots, AT9, AT10, AT11; fixture defect corrected, see addendum)
```

The driver's byte-preserved envelope under
`site/reports/integration/colima/20260923T215040310333Z/` (`integration.json`
source revision `6bca4e11fd571e95f1c97169f2d73a98561c7498`) records status
`FAIL` and is left untouched: that failure was the test fixture, not ATM. The
fixture's Hermes gateway had no configured AI provider (its own run log says
so), so the Hermes-skills step recorded one setup PASS and no provider-backed
skill reports; the task-start and assignment prompt steps completed PASS
before the run was interrupted during prompt-handoffs. The corrected fixture
and the completed checks are in the addendum below.

### Colima after the harness fix (2026-09-23, manual addendum)

The envelope's `FAIL` was a fixture defect, not a product one. Hermes
`v2026.9.21-atm` is multiplex-only and reads a profile's API keys from
`$HERMES_HOME/.env`, never from the process environment, so the gateway had
no AI provider and logged `Model resolution failed` on every turn.
atm-hermes-testbed #16 (merged, `c753aad`) writes the allowlisted key into the
profile env at bringup and adds a provider probe that quotes the gateway's own
line and stops the run. Rand's ruling the same evening ended further runs of
the timer-driven harness (redesign brief: PR #1584); the remaining evidence
was gathered by talking to the fixture agents over `atm send` and is kept raw
under `docs/plans/phase-bc/bc.7-colima-manual-evidence/`:

- `skills-20260923T224835Z/`: the testbed `test.sh` run on the fixed bringup
  (`result.txt`: `FAIL  skills reported 5/7, PASS 5/7`). The two unfilled
  slots were `atm-nudge-roundtrip` (tester, hermes); that phase was cut off
  fourteen seconds in when a second `run.sh` replaced the container.
- `nudge-roundtrip-transcript.md`: the same two sentences sent by hand to the
  live fixture; both agents announced START within 25 s and reported
  `atm-nudge-roundtrip` PASS (tester 3/3, hermes 5/5 native) within 65 s;
  the ack id the tester received is the id hermes reports sending.
- `AT11-run-prompts.log` + `prompt-AT11.json`: the prompt-handoffs step,
  absent from the driver run because that run was interrupted during it,
  run by hand on the same fixture: `VERDICT AT11: pass` (3/3 cases).

Result on ATM 1.6.1: all seven skill slots PASS, AT9 PASS, AT10 PASS, AT11
PASS. There is no single driver-recorded `colima integration: PASS` envelope
for 1.6.1; the redesigned harness produces the next one.
