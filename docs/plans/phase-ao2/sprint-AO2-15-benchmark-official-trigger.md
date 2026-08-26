# Sprint AO2.15 — `just benchmark-official`: Unattended, CI-Triggerable Benchmark Runs

Status: complete · Branch: `feature/ao2-15-benchmark-official-trigger` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: Cipher-311d · recommended_model: fast

Replaces the retired AO2.15–17 guardrails plan (parked at `105813db8` per
Rand's benchmark-infra freeze; this is the one excepted item). Goal: a
benchmark run that triggers like CI — zero agent tokens, exit-code
verdict — on the pre-provisioned `atmbench` account (rand-m5).

**Ground truth, verified live 2026-08-25** (round-1 review forced the
re-check, correctly): the account has the repo clone, pinned toolchain,
write-enabled deploy key, and the Apple Development certificate. The
"signing blocker" from the 08-22 inventory note NO LONGER EXISTS — the
account's clone is simply stale (`develop @ 53916e49c`, predating
`unique_identities` fingerprint dedupe); the CURRENT
`scripts/macos_development_signing.py` was copied to the account and
resolves the identity cleanly both with and without
`ATM_SIGNING_IDENTITY` (fingerprint `80670DBD…`, team `4869P2ZYC6`).
There is no signing deliverable; preflight's sync-to-origin cures the
staleness. **Correction (team-lead, post-AO2.14-merge)**: `just
benchmark-publish` is NOT missing — AO2.13 (merged via #1013–#1015) already
implemented it (`Justfile:234`, `scripts/smoke/benchmark_publish.py`). This
sprint's scope is Deliverables 2 and 3 only; Deliverable 1 below is retained
purely as a fixture/hardening check against the existing recipe, not new
implementation.

## Deliverables

1. **`just benchmark-publish` fixture/hardening pass** (already implemented
   by AO2.13 — this sprint only adds the D1 fixture tests below against the
   existing recipe; no new recipe is written): confirm it stages ONLY
   `site/reports/**` artifacts, runs `just reports-index --check` (already
   exists and exits nonzero on staleness), and fails without staging
   anything if the check fails. No other paths are ever staged.
2. **`scripts/smoke/benchmark_official.py` + `just benchmark-official`
   recipe** — one headless command:

```
preflight:  whoami == atmbench (or ATM_OFFICIAL_ACCOUNT override);
            daemon-switch.py status --doctor: NOTE it always exits 0 —
            health must be read from its JSON output (healthy field /
            absence of the error key), never exit-code-gated;
            git fetch; if local HEAD is AHEAD of origin/<branch>
            (stranded evidence commit from a prior failed push): attempt
            the push first, else exit 2 naming the stranded commit —
            NEVER reset it away; only then hard-sync to origin/<branch>
            (arg, default the current integrate/phase-*); clean tree.
build:      cargo build --release -p agent-team-mail -p atm-daemon,
            then .just/sign_daemon_dev.py — both fatal on nonzero
            (exit 2): an official run must measure freshly built, signed
            binaries of the synced HEAD, never leftovers.
run:        ATM_CAPACITY_HOST_LABEL=m5-atmbench — invoking
            run_admission_capacity.py DIRECTLY and interpreting ITS exit
            status (not `just benchmark`'s aggregate: the Justfile's
            report-rebuild step can mask a recorded FAIL with its own
            failure), then benchmark_report.py --rebuild as a separate
            step whose failure is exit 2, never a verdict change.
publish:    just benchmark-publish
push:       git commit (staged report artifacts) + git push, authed via
            GIT_SSH_COMMAND="ssh -i ~/.ssh/<deploy-key> -o
            IdentitiesOnly=yes -o BatchMode=yes" — explicit IdentityFile,
            agent- and login-keychain-independent, so the same command
            works under launchd's non-login context. The plist carries
            this environment.
verdict:    exit 0 = all floors green
            exit 1 = one or more FAIL (stdout names target/p50/floor;
                     best-effort `atm send team-lead` one-liner)
            exit 2 = infra error (preflight, rebuild, publish, or push
                     failure) — measured-and-published FAILs are NEVER
                     reclassified as infra errors or vice versa.

evidence:   every official invocation leaves a committed artifact on the
            selected branch: a measured invocation publishes immutable
            per-target JSON and one campaign JSON; an invocation that cannot
            reach measurement (including a trigger non-start) publishes a
            timestamped failed-attempt note under
            docs/plans/phase-ao2/evidence/. The note records trigger, branch
            and SHA when known, failure boundary, measurement-start status,
            and cleanup state. A failed-attempt note is not a benchmark
            result and cannot alter any existing campaign.
```

   No wyvern step, no prompts; unattended-safety is tested by running
   the full script with stdin redirected from `/dev/null` plus a static
   check that no `input()`/`getpass` call sites exist on this path.
   Single timestamped log under `~/benchmark-logs/`.
3. **Triggers + runbook section** (in the existing benchmark-run skill
   doc): the on-demand one-liner
   (`ssh atmbench@rand-m5.local 'cd ~/github/atm-core && just benchmark-official'`),
   a `com.atm.benchmark-official.plist` launchd template under `tools/`
   carrying the `GIT_SSH_COMMAND`/`PYO3_PYTHON`/`ATM_SIGNING_IDENTITY`
   environment, with install/uninstall one-liners (scheduling is an ops
   action). Install steps include a one-time host-key pre-trust
   (`ssh-keyscan github.com >> ~/.ssh/known_hosts`, idempotent) so the
   first unattended push cannot fail closed on an unknown host. The
   plist's `<deploy-key>` IdentityFile placeholder is resolved at INSTALL
   time by the install one-liner (sed against the account's actual key
   filename) — the committed template carries the placeholder, never a
   real path; note that a future CI-runner job is this same command.

## Acceptance criteria

1. (D1) `benchmark-publish` fixture tests: stages only `site/reports/**`
   (dirty unrelated file untouched); fails pre-staging when
   `reports-index --check` fails.
2. (D2) Exit-code contract fixtures: green campaign → 0; FAIL campaign →
   1 naming target/p50/floor (and the report-rebuild step failing AFTER
   a FAIL still exits 1 — verdict precedence test); preflight failures
   (wrong account, dirty tree, unhealthy daemon-switch, stranded-commit
   with unreachable remote) → 2 with actionable messages; stranded
   commit with reachable remote is pushed, then the run proceeds;
   stranded commit whose push is REJECTED (diverged remote) → exit 2
   naming both shas, never a reset;
   publish- or push-step failure AFTER a measured FAIL still exits 1
   (verdict precedence, same rule as rebuild); a GREEN campaign followed
   by publish/push failure exits 2 (measured PASS is not a verdict until
   published);
   missing/stale release binaries relative to synced HEAD fail closed at
   the build step (fixture: binaries absent → exit 2 before any
   measurement).
3. (D2) Unattended-safety: full script under stdin=/dev/null completes
   without hang; static scan finds no tty-read call sites on the path.
4. (D2) Push auth: the push succeeds via the explicit
   `GIT_SSH_COMMAND` identity with no agent and no unlocked login
   keychain (asserted in the launchd live run).
5. (D3) Live-verify, both triggers: one run via the ssh one-liner from
   rand-m4 and one via launchd on rand-m5, each fully unattended,
   evidence pushed, correct exit code — transcripts committed as sprint
   evidence. The launchd transcript explicitly shows zero
   interactive/keychain prompts.
6. All suites green on all three CI lanes.
7. (D3) Every official invocation leaves committed attempt evidence: measured
   invocations publish their campaign JSON; pre-measurement failures publish a
   timestamped failed-attempt note. Future `baselines.json` revisions require
   at least three clean, published official runs for the same host, target,
   and benchmark contract. A clean run has a complete target result,
   byte-exact restore/cleanup evidence, and no trigger, infrastructure, or
   harness error; failed attempts and partial runs do not count. A later
   baseline revision never rewrites older campaign snapshots.

## Required validation

- The two live unattended runs (AC #5) BEFORE quality-mgr dispatch.
- quality-mgr review of the preflight rules (what makes a run
  "official") and the stranded-commit policy.

## Non-closure / out of scope

- Everything from the retired guardrails plan (frozen; opportunistic
  findings only).
- Signing-script changes — verified unnecessary (see Ground truth).
- Scheduling policy; Windows/fastpc4 equivalent.

## Dependencies

- must_follow: AO2.13 (deliverable 3 amends its benchmark-run skill
  doc; PR-completion trigger — AO2.13 is already merged via the
  #1013–#1015 chain, so this is satisfied). Otherwise touches `Justfile`,
  `scripts/smoke/benchmark_official.py` (new), `tools/`, the skill doc;
  no runtime crates. Dispatchable immediately; parallel_safe with all
  open work.

## QA history

| Round | Reviewer | Commit | Result | Disposition |
|-------|----------|--------|--------|-------------|
| 1 | critical-plan-reviewer (sonnet, combined pass) | `3c6bd2ec6` | FAIL — 2 Blocking (signing "fix" duplicated already-landed dedupe code — live re-verification proved the real cause is a stale clone, no signing work needed at all; `just benchmark-publish` assumed to exist but was never implemented), 3 Important (Justfile rebuild step can mask a recorded FAIL's exit status; hard-sync could silently discard a stranded evidence commit; no SSH auth contract for launchd's non-login push), 2 minor (tty-test mechanism; "daemon-switch sane" undefined) | Fixed in round-1 rewrite: signing deliverable deleted with live evidence recorded (current resolver passes with/without override on the atmbench keychain); `benchmark-publish` promoted to deliverable 1; glue invokes the runner directly with verdict-precedence rule + fixture; stranded-commit push-first/fail-closed preflight; explicit `GIT_SSH_COMMAND` IdentityFile contract carried by the plist; stdin=/dev/null + static-scan tty test; `daemon-switch.py status --doctor` healthy named. |
| 2 | critical-plan-reviewer (sonnet) | `de8eb1d25` | FAIL — 6/7 round-1 items verified resolved (incl. the empirical signing deletion); 1 new Important: the direct-runner rewrite dropped the cargo-build + sign steps, risking measurement of stale binaries | Fixed in round-2 commit: explicit build stage (cargo build --release + sign_daemon_dev.py, fatal exit 2) restored ahead of the runner, with a missing-binaries fail-closed fixture in AC #2. |
| 3 | critical-plan-reviewer (sonnet) | `47e6d4d59` | **PASS** — build-stage restoration verified; zero findings; hardening complete | Ready for quality-mgr final QA. |
| 4 | quality-mgr gate (PR #1022) | pre-fix head | **PASS**, 0 Blocking — 6 doc-level pre-merge fixes (must_follow AO2.13; two missing verdict-precedence AC rows; SSH host-key pre-trust; daemon-switch --doctor always exits 0 so preflight must be a JSON-content check; rejected-push stranded fixture; plist placeholder timing) + 1 refuted req-qa false positive (wrong-branch signing claim) | All six applied in this commit; no new QA round required per team-lead. |
| 5 | team-lead doc correction (post-AO2.14-merge, pre-dev-dispatch) | this commit | N/A (doc-only correction, not a QA round) | Deliverable 1's framing was stale: `just benchmark-publish` was described as "not yet implemented... built here," but AO2.13 (#1013–#1015) already shipped it (`Justfile:234`, `scripts/smoke/benchmark_publish.py`), flagged by quality-mgr at round 4. Corrected: D1 is now scoped as a fixture/hardening pass against the existing recipe, not new implementation. Dev dispatch scope is D2 + D3 (+ D1 fixtures). |
