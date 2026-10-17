# Intent - phase-bi: install the skill from atm-bd-console

## What this plan is for

One install process that installs `atm-bd-console` for the user and also lets
them install the atm-bd-orchestration skill into a repository. Today the app
installs with `uv tool install` but the skill needs a separate hand-run
`install.py`. This phase puts the skill install behind an `atm-bd`
subcommand that does what `install.py` does now.

## What success looks like

- `atm-bd skill` installs the skill into a repository with the same
  result as `install.py`.
- `atm-bd` is the one command: `atm-bd console` (the TUI), `atm-bd skill` (the
  install) and `atm-bd cron --phase <x>` (the headless tick). `atm-bd-console`
  is dropped, not aliased.
- `atm-bd skill` installs the skills bundled in the app; `atm-bd skill x.y.z`
  installs a release tag, `--latest` installs the head of `main` (a skill patch
  ships without republishing the app), `--list` shows what the repo offers, and
  `--orchestration` names today's skill set.
- The phase ends with the app published to PyPI.

## What is deliberately out of scope

- Any skill set other than today's (for example `atm-bd-lite`).
  A later phase adds them; this one does not design them.
- Anything that changes what ships in v0.16.1.
