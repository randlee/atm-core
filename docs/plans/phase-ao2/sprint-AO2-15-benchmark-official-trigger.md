# Sprint AO2.15 — `just benchmark-official`: Unattended, CI-Triggerable Benchmark Runs

Status: draft · Branch: `feature/ao2-15-benchmark-official-trigger` off
`integrate/phase-ao2` · PR target: `integrate/phase-ao2`
recommended_agent: Cipher-311d · recommended_model: fast

Replaces the retired AO2.15–17 guardrails plan (parked in git history at
`4e84b41bf`..`105813db8` per Rand's benchmark-infra freeze, 2026-08-25).
This sprint delivers the ONE excepted item: a benchmark run that triggers
like CI — zero agent tokens, exit-code verdict — using infrastructure Rand
already provisioned on the `atmbench` account (rand-m5): repo clone,
pinned toolchain, **write-enabled deploy key**, and the Apple Development
certificate (verified live 2026-08-25: identity `80670DBD…` valid,
`ATM_SIGNING_IDENTITY` override set in `~/.zprofile`).

## Deliverables

1. **Signing-script fix** (`.just/sign_daemon_dev.py`): the sole remaining
   blocker on the atmbench account. The script requires exactly one
   identity match but the account's keychain search list echoes the one
   real certificate 4× (default search list crossing the hidden
   "Local Items"/iCloud store). Fix the whole defect class:
   (a) dedupe candidate identities by hash before the exactly-one check;
   (b) when `ATM_SIGNING_IDENTITY` is set, select by that hash and error
   only if it is absent/invalid. Actionable errors name the hashes seen.
2. **`just benchmark-official` recipe** (+ `scripts/smoke/benchmark_official.py`
   for the logic): one command, headless, chaining ONLY existing pieces:

```
preflight:  whoami == atmbench (or ATM_OFFICIAL_ACCOUNT override),
            clean tree, expected branch (arg, default integrate/phase-*),
            git fetch + hard sync to origin/<branch>,
            daemon-switch state sane
run:        ATM_CAPACITY_HOST_LABEL=m5-atmbench just benchmark
publish:    just benchmark-publish && just reports-index --check
push:       git commit (report artifacts only) + git push (deploy key)
verdict:    exit 0 = all floors green
            exit 1 = one or more FAIL (stdout names target/p50/floor;
                     best-effort `atm send team-lead` one-liner)
            exit 2 = infra error before measurement (nothing published,
                     per the no-noise rule — fix and rerun silently)
```

   No wyvern step, no prompts, no interactive auth anywhere; single
   timestamped log under `~/benchmark-logs/`.
3. **Triggers + runbook section** (in the existing benchmark-run skill
   doc, not a new doc): the on-demand one-liner
   (`ssh atmbench@rand-m5.local 'cd ~/github/atm-core && just benchmark-official'`),
   a `com.atm.benchmark-official.plist` launchd template committed under
   `tools/` with install/uninstall one-liners (scheduling itself remains
   an ops action), and the note that any future CI runner job is just
   this same command.

## Acceptance criteria

1. (D1) Fixture tests: duplicated identity listing (same hash 4×) passes
   the exactly-one check; two genuinely distinct hashes without override
   fails naming both; override selecting an absent hash fails actionably.
   Live proof: `just build` on the atmbench account produces a daemon
   whose `codesign -dv` shows the real Apple Development authority — not
   ad-hoc.
2. (D2) Preflight failures exit 2 with actionable messages (wrong
   account, dirty tree, wrong branch — each fixture-tested); a FAIL
   campaign exits 1 naming target/p50/floor; a green campaign exits 0.
   Zero interactive prompts under `ssh -o BatchMode=yes` (test asserts no
   tty reads).
3. (D2) The publish step pushes via the account's deploy key: evidence
   commit visible on origin after an unattended run; nothing outside
   `site/reports/` is ever staged (dirty-unrelated-file fixture).
4. (D3) Live-verify, both triggers: one run started via the ssh one-liner
   from rand-m4 and one via launchd on rand-m5, each completing
   unattended with evidence pushed and correct exit codes — transcript
   committed as sprint evidence.
5. All suites green on all three CI lanes.

## Required validation

- The two live unattended runs (AC #4) BEFORE quality-mgr dispatch.
- quality-mgr review of the preflight rules (what makes a run "official").

## Non-closure / out of scope

- Everything from the retired guardrails plan (contract hash, ancestry
  rule, host allowlist, hot-path lint, tripwire) — frozen; opportunistic
  findings only.
- Scheduling policy (nightly vs weekly) — ops decision, plist is agnostic.
- Windows equivalent (fastpc4) — follow-on only if Rand asks.

## Dependencies

- must_follow: none — touches only `.just/sign_daemon_dev.py`,
  `scripts/smoke/benchmark_official.py` (new), Justfile, `tools/`, and
  the skill doc; no runtime crates, no intersection with AO2.14's fix
  round or any open sprint. Dispatchable immediately.
- parallel_safe: all open work.

## QA history

| Round | Reviewer | Commit | Result | Disposition |
|-------|----------|--------|--------|-------------|
