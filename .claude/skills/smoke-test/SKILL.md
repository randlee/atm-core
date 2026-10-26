---
name: smoke-test
description: Run or inspect the repo-native smoke harness for atm-core. Use when implementing or operating `just smoke`, including live `localhost` and `local-ip` hardware checks; when rendering or inspecting `site/reports/smoke/*`; or when triaging smoke findings against retained evidence.
---

# Smoke Test Skill

Use this skill for the repo-native smoke harness and its report artifacts.

## Start Here

- Read `references/level-matrix.md` to select `fast`, `normal`, or `thorough`.
- Read `references/phase-z-row-map.md` when you need the frozen row IDs and
  level-to-row coverage expectations.
- Read `references/report-schema.md` when you need the canonical evidence
  layout and JSON contract.

## Live daemon prerequisite

Before any live `just smoke localhost`, `local-ip`, `peer-preflight`,
`crosshost-send`, or `crosshost-ack` run, use the
[`/daemon-switch`](../daemon-switch/SKILL.md) skill. It selects the matching
CLI and daemon as one pair, restarts only the one managed daemon, and requires
`atm doctor --json` to report the selected pair ready. Do not manually start a
daemon, point a service at a worktree binary, or run smoke with a split
CLI/daemon pair.

## Isolated test account on a shared host (`m5-atmbench`)

`ssh atmbench` is a test-only account on the same M5 as `randlee`. Files left
in it by earlier runs are disposable. The `randlee` daemon owns direct-peer
port 43101, and an ssh session has no GUI login, so the account's LaunchAgent
cannot load and daemon-switch cannot manage it. This is the only place where
the daemon is started by hand. Set it up as follows:

1. **Latest tools.** Test with the newest release of every dependency, so a
   breaking change shows up here first:
   - the ATM prerelease under test, unpacked to `~/.atm-builds/vX.Y.Z` with
     the account-local selectors in `~/.atm-builds/selectors/darwin`;
   - the latest `herdr` release binary in `~/.atm-builds/tools/bin`;
   - `just bootstrap`, so `.bootstrap-venv` carries the pinned sc-compose.

   The account has no `gh` login. Download prerelease assets with `curl` from
   `https://github.com/randlee/atm-core/releases/download/prerelease%2FvX.Y.Z/`
   and verify them against `checksums.txt` (`shasum -a 256 -c`). Never write
   to `/opt/homebrew/bin`: it is shared with `randlee`.
2. **Environment** for every command:
   `PATH=$HOME/.atm-builds/tools/bin:$HOME/.atm-builds/selectors/darwin:/opt/homebrew/bin:$PATH`
   and `PYO3_PYTHON=/opt/homebrew/bin/python3.11`.
3. **Peer identity, once per clean `~/.atm`.** Use port 43201:
   - Generate a self-signed `rand-m5.local`/`localhost`/`127.0.0.1` bundle in
     `~/.atm/peer-identity/atmbench.bundle.pem` with `openssl req -x509`.
   - `atm peer certificate init --fingerprint <sha256> --private-key-ref <bundle> --yes`
   - `atm peer interface set --bind 0.0.0.0:43201 --advertise-host rand-m5.local --enabled`
   - `atm peer trust add --host localhost --fingerprint <sha256> --https-port 43201 --yes`
4. **Start:**
   `env -u ATM_TEAM -u ATM_IDENTITY -u ATM_ENVIRONMENT ~/.atm-builds/selectors/darwin/atm-daemon --direct-peer-port 43201`
   (nohup, logs in `~/.atm/daemon/`), then confirm `atm doctor --json` is
   healthy. If the daemon aborts with no output (exit 134), another listener
   holds its direct-peer port.
5. **Run:**
   `ATM_TEAM=atmbench-smoke ATM_IDENTITY=smoke-lead ATM_SMOKE_DIRECT_PEER_PORT=43201 just smoke localhost`
6. **Stop** the daemon afterwards (`pkill -TERM -u atmbench -f atm-daemon`).

Known gap: outbound peer sends still dial port 43101 (#2121), so the localhost
send rows fail here until that is fixed. The doctor row is valid.

## Implementation Surface

- runner:
  - `scripts/smoke/run.py`
  - `scripts/smoke/run_feature_smoke.py`
- retained-log analysis:
  - `scripts/smoke/analyze_logs.py`
- fixture and artifact path helpers:
  - `scripts/smoke/fixtures.py`
- markdown rendering:
  - `scripts/smoke/render_report.py`

## Artifact Rules

- Live hardware smoke uses the fuzz-style, self-contained report layout:
  `site/reports/smoke/<platform>/<host>/<run-id>-pid<PID>-<feature>/`.
- The directory contains the JSON result, per-host XHTML panels, feature HTML,
  its own `index.html`, and a `smoke.envelope.json` registration record. The
  generated master navigation at `site/reports/index.html` links every run;
  no smoke payload or panel is written to the site root or top-level
  `site/reports` directory.
- `platform` is the operating-system label (`macos`, `windows`, or `linux` as
  reported by the runner), `host` is the sanitized local hostname, and
  `run-id` is `ATM_SMOKE_RUN_ID` when supplied or a UTC microsecond timestamp.
  The PID suffix preserves non-overlap even for simultaneous same-host runs.
- Report consumers must use the path printed by `just smoke`; never infer a
  shared “latest” artifact.
- Fixture levels (`fast`, `normal`, `thorough`) use the same platform/host/run
  directory layout and retain their Markdown and JSON inside that directory.

## Notes

- `fast` is the quick clean-room happy-path lane
- `normal` is the default `just smoke` lane
- `thorough` is the full CLI plus common-error-path lane
- the real same-host `atm-graft` advisory and unary ICD lane is the explicit
  managed-profile command `just smoke graft-hermes`; it is not part of the
  clean-room `thorough` fixture because that fixture must never start the
  frozen legacy daemon
- shared fixture labels are sprint-scoped on purpose:
  - `fast` uses `z19-team`
  - `normal` uses `z20-team`
- current daemon shutdown handling in the smoke runner is POSIX-only; fail
  closed on non-POSIX hosts instead of pretending Windows support exists
- major findings promote into `docs/plans/phase-Z/smoke-findings-review.md`
