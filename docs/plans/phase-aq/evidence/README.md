# Phase AQ live-evidence job

`.github/workflows/phase-aq-evidence.yml` runs the Phase AQ live-evidence
harnesses — AQ1.9's restart matrix, AQ2.5's queue-delivery-trigger scenario,
and any future AQ3 drain/sweep transcript harness — on a clean GitHub Actions
runner instead of a developer host.

## Why local runs fail closed

Every one of these harnesses drives a real, owned `atm-daemon` process
against the real `atm` CLI. The daemon runtime and database are intentionally
**OS-account scoped**: `atm_core::home::current_host_runtime_scope` ignores
`ATM_HOME`, `HOME`, and the current directory by design, and
`DaemonOwnerGuard` enforces that scope with an OS-level exclusive file lock so
two daemons can never race the same host account.

That is correct behavior in production, but it means every harness refuses to
start whenever a developer's own ambient `atm-daemon` already owns that lock
— which is the normal state of any workstation actively dogfooding ATM. The
harnesses do not fake or skip this check; they record a
`blocked_ambient_daemon` (or `PENDING-DEDICATED-HOST-RUN`) status and exit,
exactly as designed, rather than risk contending with the ambient session.
See `scripts/phase-aq/run_hermes_atm_restart_matrix.py` and
`scripts/phase-aq/run_aq25_queue_delivery_trigger_evidence.py` for the exact
check (`ambient_daemon_pids()` / `require_clean_host()`).

A hosted GitHub Actions runner has no such ambient daemon. It is a clean,
single-use OS account for the duration of the job, so it can produce real
positive-path evidence where a developer host structurally cannot.

## How to trigger the job

Dispatch it manually against the PR branch that needs live evidence:

```bash
gh workflow run phase-aq-evidence.yml \
  --ref integrate/phase-aq \
  -f branch=<pr-branch-name>
```

For example, to (re)run evidence for the AQ2.5 branch:

```bash
gh workflow run phase-aq-evidence.yml \
  --ref integrate/phase-aq \
  -f branch=feature/aq-2-5-queue-delivery-triggers
```

The job also runs automatically on any pull request that touches
`scripts/phase-aq/**` or `docs/plans/phase-aq/evidence/**`, using the PR's own
head commit. That automatic run is a correctness check on the harness itself
(does it still build, install, and execute cleanly) — it uploads whatever
transcripts it produces as workflow artifacts but does not commit them
anywhere, since a pull-request-triggered run has no authority to push to the
PR branch.

The job runs on both `ubuntu-latest` and `macos-latest`. The macOS leg exists
because AQ3's drain/sweep transcript exercises a real tmux pane, and `tmux` is
not preinstalled on the macOS runner image (the job installs it via
`brew install tmux` when missing).

## How evidence lands

1. The job builds the workspace in release mode, installs the `atm_graft`
   Python extension into the repository's `.bootstrap-venv` via
   `maturin develop` (mirroring `scripts/test_atm_graft_python.py`), confirms
   the runner has no ambient `atm-daemon`, and then runs every
   `scripts/phase-aq/run_*.py` harness present on the checked-out branch that
   accepts an `--evidence-dir` flag. A harness that has not landed yet on that
   branch (for example, an AQ3 drain/sweep transcript runner that does not
   exist at the time of writing) is skipped with a clear `SKIP:` log line —
   the job does not hardcode a fixed script list, so a differently named
   harness that lands later is picked up automatically.
2. Each harness writes its own JSON + Markdown transcript under
   `docs/plans/phase-aq/evidence/<sprint>/…` (for example `AQ1.9/` or
   `AQ2.5/`), exactly as it does when run locally.
3. The job uploads the full `docs/plans/phase-aq/evidence/` tree as a workflow
   artifact named `phase-aq-evidence-<os>` for each matrix leg.
4. **Committing the transcripts back is a manual step.** Opening or updating a
   pull request that commits these transcripts onto the dispatched branch
   would need a token with `pull-requests: write` scope against this
   repository. Neither the default `GITHUB_TOKEN` convention used elsewhere in
   this repo's workflows (see `.github/workflows/release.yml` and
   `.github/workflows/release-preflight.yml`, which only use `contents: write`
   plus external registry secrets — never a PR-creation token) nor any
   repository secret currently grants that. Rather than invent a new token or
   silently no-op, the job stops at the artifact upload and prints the exact
   manual recovery command in its job summary. Reproduced here:

   ```bash
   # Download the transcripts produced by a dispatched run.
   gh run download <run-id> --name phase-aq-evidence-ubuntu-latest --dir /tmp/phase-aq-evidence
   # (repeat with phase-aq-evidence-macos-latest if that leg produced
   # evidence you also want, e.g. AQ3's tmux transcript)

   # Land them on the branch that needs the evidence.
   cp -R /tmp/phase-aq-evidence/. docs/plans/phase-aq/evidence/
   git checkout -b evidence/<branch>-<run-id> origin/<branch>
   git add docs/plans/phase-aq/evidence
   git commit -m "docs(aq): live-evidence transcripts from run <run-id>"
   git push -u origin evidence/<branch>-<run-id>
   gh pr create --base <branch> --title "docs(aq): live-evidence transcripts (run <run-id>)" --fill
   ```

   If a future sprint wires up a repository-scoped PR-creation token, this
   workflow's final step (`Print manual evidence-commit instructions`) is the
   place to replace with an actual `gh pr create` / `peter-evans/create-pull-request`
   call.
