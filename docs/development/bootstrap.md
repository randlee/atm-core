# Pinned bootstrap

`just bootstrap` is the sole repository-owned installation contract for the
tools used by the normal CI lint/test lanes and by an isolated benchmark OS
account. Its complete, exact version set is in
[`tools/bootstrap.toml`](../../tools/bootstrap.toml); Python's exact dependency
closure is in [`tools/bootstrap-requirements.txt`](../../tools/bootstrap-requirements.txt).

The recipe deliberately does not select a latest release, a version range, or
an M5-specific installation path. It installs the manifest's exact Cargo tools
and source revision, creates a repository-local `.bootstrap-venv`, installs the
exact Python packages there with `--no-deps`,
then verifies every reported version.

## Seed contract

`just` cannot install itself and Python must exist before a Python recipe can
run. GitHub CI supplies these exact seed tools before it invokes the shared
recipe:

- Rust `1.94.1`, with `clippy` and `rustfmt`;
- Python `3.14.7`; and
- `just` `1.58.0`.

On a local macOS account, `just bootstrap` makes this seed contract
reproducible through Homebrew: if Python or `just` differs from the exact
manifest value, it installs/upgrades only `python@<manifest major.minor>` and
`just`, then re-executes through the Homebrew Python. Homebrew is preferred
because it keeps the host on its current stable bottle as pins advance; the
bootstrap still rejects a Homebrew release that does not exactly match the
manifest. CI is excluded from this host-package action and continues to
provision its own seeds explicitly.

An operator provisioning a non-macOS benchmark account must supply the same
three exact versions and run the same command from a clean checkout. If any
seed version is different, the recipe refuses before it installs or modifies
any dependent tool. The bootstrap never writes packages into the system Python;
every other Python-backed `just` recipe runs through `.bootstrap-venv` after setup. Its
`bin`/`Scripts` directory is also first on child-process `PATH`, so PyO3 and
other helpers cannot accidentally select an unrelated account-level Python.

On macOS, the recipe includes `/opt/homebrew/bin` while locating `python3.14`.
That makes the same command work from a non-interactive account whose shell
does not source the interactive Homebrew PATH setup; the bootstrap still
rejects any patch release other than the manifest's exact Python version.

For review without changing tools, run:

```sh
just bootstrap --dry-run
```

This is a tool-environment operation only. It does not build ATM, sign a
binary, run `daemon-switch`, start a daemon, or touch an ATM database.

## Version-selection policy

Every manifest entry pins the newest stable release compatible with the
repository's pinned Rust and Python baselines. The current exceptions are
`cargo-shear` `1.12.0` (upstream `1.13.4` requires Rust `1.95.0`) and
`cargo-modules` `0.26.0` (upstream `0.27.0` requires Rust `1.95.0`), while this
repository deliberately pins Rust `1.94.1`. Compatibility exceptions must be
documented here and re-evaluated whenever the seed Rust version changes.
