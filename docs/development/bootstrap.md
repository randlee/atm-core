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
run. A runner therefore supplies these exact seed tools before it invokes the
shared recipe:

- Rust `1.94.1`, with `clippy` and `rustfmt`;
- Python `3.14.7`; and
- `just` `1.58.0`.

GitHub CI provisions those seeds explicitly and then runs `just bootstrap`.
An operator provisioning a benchmark account must supply the same three exact
versions and run the same command from a clean checkout. If any seed version is
different, the recipe refuses before it installs or modifies any dependent
tool. The bootstrap never writes packages into the system Python; every other
Python-backed `just` recipe runs through `.bootstrap-venv` after setup. Its
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
