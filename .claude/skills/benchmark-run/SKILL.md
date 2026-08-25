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
dedicated account. The ordinary benchmark harness uses its own temporary ATM
home and daemon. The official runner first terminates any `atm-daemon` owned
by the disposable benchmark account—never a different OS user's daemon—then removes only that manifest-verified
account's `.atm/db` and `.atm/benchmark-snapshots` directories. It retains the
manifest and published evidence. `daemon-switch status --doctor` and `atm
doctor --json` are useful account-health diagnostics, not a prerequisite for
the harness.
On macOS, `just benchmark` signs its fresh release binaries first. A dedicated
benchmark account with a locked Apple Development keychain must be provisioned
with its account-local, untracked keychain secret; the signing helper unlocks
it without printing the secret or prompting the operator.

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

## 8. Official unattended M5 trigger

Use this only from the dedicated `atmbench` account on M5. It is the
headless counterpart to sections 1–4: it verifies the account and checkout,
syncs the current `integrate/phase-*` branch, builds and signs fresh release
binaries, then lets the ordinary matrix runner create and tear down its
isolated temporary daemon and ATM home. It never requires, checks, selects,
or preserves an ambient account daemon: it terminates one belonging to
`atmbench` before and after the run, and deletes the account's disposable
database plus benchmark snapshots after use. It rebuilds reports, stages only
public artifacts, commits, and pushes. The runner derives `GIT_SSH_COMMAND` at
runtime from the account-local `ATM_BENCHMARK_DEPLOY_KEY`, so the LaunchAgent
does not carry a duplicate static SSH command. Its exit code is the
machine-readable result:
`0` means all target floors passed, `1` means a measured campaign was published
with one or more FAIL targets, and `2` means no publishable official result was
produced because of an infrastructure error.

`just benchmark-official` runs the repository's pinned `just bootstrap` first,
so the dedicated account needs no pre-existing Python virtual environment.

On-demand from M4:

```sh
ssh atmbench@rand-m5.local 'export PATH="$HOME/.local/bin:/opt/homebrew/bin:$PATH"; cd ~/github/atm-core && just benchmark-official'
```

To live-validate a reviewed PR before it is merged, name that remote branch
explicitly. This is the only supported exception to the ordinary
`integrate/phase-*` default and retains all evidence on the reviewed branch:

```sh
ssh atmbench@rand-m5.local 'export PATH="$HOME/.local/bin:/opt/homebrew/bin:$PATH"; cd ~/github/atm-core && just benchmark-official --branch feature/<reviewed-branch>'
```

For a LaunchAgent, first set the two account-local values without committing
them: `ATM_BENCHMARK_DEPLOY_KEY` must name the deploy key that has write access
to the repository, and `ATM_SIGNING_IDENTITY` must be the installed Apple
Development identity. Pre-trust GitHub once so the first launchd push cannot
wait for a host-key prompt; the command is idempotent:

```sh
ssh-keygen -F github.com >/dev/null || ssh-keyscan -H github.com >> ~/.ssh/known_hosts
```

Install the committed template with concrete paths substituted only in the
account-local copy, then invoke it. The template deliberately contains no key
path, identity fingerprint, account secret, or repository-specific home path.

```sh
key="${ATM_BENCHMARK_DEPLOY_KEY:?set the account-local deploy-key path}"
identity="${ATM_SIGNING_IDENTITY:?set the Apple Development identity}"
repo="$HOME/github/atm-core"
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/benchmark-logs"
sed -e "s|__ATM_REPO__|$repo|g" -e "s|__ATM_DEPLOY_KEY__|$key|g" \
  -e "s|__ATM_SIGNING_IDENTITY__|$identity|g" -e "s|__ATM_HOME__|$HOME|g" \
  "$repo/tools/com.atm.benchmark-official.plist" \
  > "$HOME/Library/LaunchAgents/com.atm.benchmark-official.plist"
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.atm.benchmark-official.plist"
launchctl kickstart -k "gui/$(id -u)/com.atm.benchmark-official"
```

Inspect the timestamped `~/benchmark-logs/benchmark-official-*.log` log and
the launchd stdout/stderr logs after it completes. Remove the trigger without
touching its benchmark evidence via:

```sh
launchctl bootout "gui/$(id -u)/com.atm.benchmark-official" || true
rm -f "$HOME/Library/LaunchAgents/com.atm.benchmark-official.plist"
```
