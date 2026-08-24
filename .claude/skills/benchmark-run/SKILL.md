---
name: benchmark-run
description: Run, review, and publish the complete ATM benchmark matrix safely and consistently.
---

# Benchmark run

This is the sole operator procedure for ATM benchmark campaigns. It supersedes
the procedural portions of AI.40, AI.49, AI.52, AL.9, AO2.5.4, AO2.7, and
AO2.8; those sprint documents retain their historical decisions and evidence.

## 1. Preflight

Run only from the dedicated benchmark OS account. Do not use a developer
account or an account with existing ATM state. Bootstrap is one-time per clean
benchmark account and refuses pre-existing state.

macOS and Linux:

```sh
git status --short
python3 .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor
atm doctor --json
test -f "${ATM_HOME:-$HOME/.atm}/benchmark-account.json" || just benchmark-bootstrap
```

Windows PowerShell:

```powershell
git status --short
python .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor
atm doctor --json
if (-not (Test-Path (Join-Path ($env:ATM_HOME ?? "$HOME\\.atm") "benchmark-account.json"))) { just benchmark-bootstrap }
```

The working tree must be clean before the run. Bootstrap only when the account
manifest is absent; an existing manifest is the expected state for a reusable
dedicated account. `daemon-switch status --doctor` and `atm doctor --json`
must identify one healthy matched CLI/daemon pair. On macOS, `just benchmark`
signs the release daemon before it is selected; do not run a worktree daemon
outside daemon-switch.

## 2. Run

The ordinary command runs the complete required matrix: `sqlite`, `uds`,
`tcp`, and `tcp-tls` at f8. Windows omits `uds`; it runs `sqlite`, `tcp`, and
`tcp-tls`. Do not use `--target` or `--diagnostic-only` for a release campaign.

macOS M5:

```sh
ATM_CAPACITY_HOST_LABEL=rand-m5 just benchmark
```

Windows PowerShell:

```powershell
$env:ATM_CAPACITY_HOST_LABEL = 'windows-x64-01'
just benchmark
```

The runner writes per-target JSON, envelopes, campaign JSON, and raw local
traces. A measured below-baseline campaign returns non-zero, but its complete
result is still rendered and must be published.

## 3. Review

Immediately after every run, display the newest panel for the operator:

```sh
just benchmark-show
```

On Windows, invoke the same `just benchmark-show` command in PowerShell. The
command rebuilds from committed-format JSON, makes an HTML twin, and opens it
in Wyvern. Confirm the host, source revision, all required targets, and each
PASS/FAIL/INCOMPLETE status before publishing.

## 4. Publish

Publish every measured campaign after review, including a FAIL or INCOMPLETE
campaign. The helper stages only report artifacts and verifies the reports
index; it does not stage source or unrelated working-tree changes.

```sh
just benchmark-publish
git commit -m "evidence(benchmark): <campaign-id> <host-label>"
git push
```

Use the same commands in Windows PowerShell. `just benchmark-publish` fails if
`just reports-index --check` is not clean. Do not amend or delete an older
campaign to make a result look better.

## 5. INCOMPLETE campaigns

An INCOMPLETE campaign is a durable record of what happened, not evidence of
performance. Run the review and publication steps exactly as above. Its reason
note must remain in the campaign artifact. Correct the harness or environment,
then run a new campaign; never rewrite or remove the incomplete one ad hoc.

## 6. Failure classification and rerun policy

- A harness error before measurement is not a campaign result: root-cause and
  fix it, then rerun the command silently.
- A measured FAIL is a campaign result: review and publish it, then root-cause
  the performance loss on a separate fix branch.
- A failure of snapshot/restore or reports-index is unsafe: stop before any
  new run, restore the benchmark account, and fix that safety mechanism first.
- Never run a benchmark against a developer account, shared account, or
  primary ATM database.

## 7. Windows appendix

Windows uses the three-target matrix (`sqlite`, `tcp`, `tcp-tls`) because Unix
domain sockets are unavailable. Use the dedicated benchmark account and
`ATM_CAPACITY_HOST_LABEL=windows-x64-01`. The paired CLI and daemon must be
selected through daemon-switch's two selector symlinks, never by replacing an
installed executable:

```powershell
python .claude/skills/daemon-switch/scripts/daemon-switch.py switch `
  --cli-link C:\\atm-active\\atm.exe --daemon-link C:\\atm-active\\atm-daemon.exe `
  --cli target\\release\\atm.exe --daemon target\\release\\atm-daemon.exe --yes `
  --service atm-daemon
python .claude/skills/daemon-switch/scripts/daemon-switch.py status --doctor
```

After the campaign, restore the installed pair through daemon-switch and run
`atm doctor --json` again. Do not create a second service or point the service
directly at a worktree executable.
