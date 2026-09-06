---
title: "Phase AX pre-merge live proof"
phase: AX
status: release_readiness
branch: integrate/phase-ax
worktree: ../atm-core-worktrees/integrate/phase-ax
---

# Phase AX pre-merge live proof

This is the operational procedure for a candidate already integrated on
`integrate/phase-ax`.  It records performance and smoke evidence; it does not
authorize source changes, database-state substitution, trust edits, or daemon
remodeling.

## Preconditions

1. Fetch the candidate and record both `HEAD` and
   `origin/integrate/phase-ax`; they must equal the approved candidate SHA.
2. Create two clean worktrees from that SHA before measurement:
   - an evidence worktree/branch, which retains only runner-generated reports
     byte-for-byte and is pushed without a pull request; and
   - a separate `/sc-git-worktree` remediation branch, used only when a
     benchmark result or harness failure needs root-cause and correction.
     Never put source edits on the evidence branch.
3. Physical performance evidence runs as the runner-provisioned dedicated
   account over `ssh atmbench@rand-m5.local`, not from the interactive rand-m4
   account.  The running agent may provision this disposable account when it
   is absent; confirm `whoami` is `atmbench` and
   `~/.atm/benchmark-account.json` exists before running a benchmark.  Do not
   bootstrap or reset another account.

## Benchmark gate

From the clean candidate worktree under `atmbench`:

```bash
git rev-parse HEAD
ATM_CAPACITY_HOST_LABEL=m5-atmbench just benchmark
just benchmark-report
just benchmark-publish
```

The evidence path is fixed and exclusive: run only
`ATM_CAPACITY_HOST_LABEL=<host>-atmbench just benchmark`, then
`just benchmark-report` (which renders
`templates/benchmark-report/benchmark-run.xhtml.j2`), then
`just benchmark-publish`.  The per-lane result JSON, the
`<campaign>.campaign.json`, and the `<campaign>.xhtml` must all be committed
under `site/reports/send-message-benchmark/`.  Hand-written or ad-hoc XHTML or
JSON is never accepted as benchmark evidence.

`just benchmark` is one complete release matrix, producing reviewed results
for sqlite, UDS, TCP, and TCP-TLS.  The selected `--transport` and `--target`
forms are diagnostic-only and do not meet this gate.  Compare each candidate
target to the newest same-host, same-target, matched-profile artifact and the
recorded floor.  Report p50, p95, requests per second, artifact path, and the
PASS/FAIL status for every target.

The runner starts, stops, snapshots, and restores only its own disposable
benchmark-account daemon.  Do not stop or restart an ambient dogfood daemon:
the benchmark's account isolation is specifically intended to avoid that
cross-account interference.

### Failure handling: root-cause before escalation

A red result is not a terminal status report.  Preserve its generated artifact
and use the pre-created remediation worktree to do the following before asking
for operator help:

The running benchmark agent owns this full loop: it reports minor blockers it
fixes itself, is empowered to make any change necessary to meet or exceed the
benchmark floors, and fixes benchmark regressions itself as part of the same
run.  It does not escalate to Rand or start a separate agent dispatch loop for
these fixes.  The benchmark run is never interrupted or second-guessed
mid-run; quality-mgr and team-lead judge the correctness of the fixes
afterward through normal PR review.  The agent must preserve the candidate,
evidence, and account-isolation rules below while doing so.

1. Reproduce the failing target locally with the same candidate, account,
   target/profile, and release build; distinguish a measured floor regression
   from bootstrap, signing, report, account, or environmental failure.
2. Inspect the candidate diff and the retained raw/result evidence; identify
   the responsible code path or prove that the failure is outside the
   candidate.
3. If it is a code regression, implement the smallest corrective change on
   the remediation branch, run the relevant local tests plus formatting and
   clippy, commit/push the fix through normal review, and advance the
   candidate only after the fix lands.
4. Rebuild and rerun the complete matrix on that new exact candidate.  Keep
   every prior failing artifact; never overwrite, hand-edit, or relabel it as
   a pass.

Escalate immediately only for an external authority/input gap (for example
missing benchmark-host access, unavailable account, absent signing identity,
or a required peer-pair configuration).  A report that merely repeats a
minor blocker or failing score without this due diligence is incomplete.

## Smoke gate

Run fixture smoke independently from the benchmark-account workflow:

```bash
just smoke normal
```

For peer-pair, an approved host-supplied role configuration and output location
are mandatory:

```bash
just smoke peer-pair --config <approved-role-config> --evidence-dir <output-dir>
```

There is no bare or default loopback peer-pair command.  Never invent
identities, trust entries, certificates, addresses, or a role configuration to
make the lane execute.  If the inputs are unavailable, report the lane as not
run and name the missing inputs.

## Completion report

Report the candidate SHA, execution host/account and OS/architecture, Herdr
version or its unavailability, all report paths, the benchmark comparison
table, each smoke result, evidence branch SHA, and one explicit verdict:
`NO REGRESSION`, `REGRESSION`, or `COULD NOT RUN`.  State a precise preflight
or input blocker instead of changing execution mode to obtain a green result.
Send the completion report directly to the requestor using the requestor's
full host-qualified address (`<agent>@<team>.<host>`) exactly as given in the
task assignment.  A bare agent name resolves to the runner's own host and may
silently misroute the report; task assignments must provide the full address.
