# Installation and troubleshooting

This reference is loaded only when the dependency preflight fails.  The
graph-orchestration entry point is intentionally fail-closed: do not dispatch
an agent or mutate phase TTL while a required dependency is missing or below
the minimum version.

## Check first

From the repository root, run the preflight before any graph query or event
write:

```bash
.claude/skills/graph-orchestration/scripts/preflight
```

The command emits a JSON response with `success`, `data.checks`, and `error`.
Exit `0` means all runtime checks passed.  Exit `2` means an operational
dependency error; inspect the check details and fix the environment.  For the
test suite, include pytest:

```bash
.claude/skills/graph-orchestration/scripts/preflight --for-tests
```

The required runtime dependencies are:

| Dependency | Minimum | Used for |
|---|---:|---|
| `sc-compose` CLI | **1.2.0** | Rendering fenced Jinja templates and agent prompts |
| `sc_compose` Python binding | **1.2.0** | Python/maturin rendering integrations and wrappers |
| Python 3 + `rdflib` | supported Python 3 | RDF/Turtle parsing and SPARQL queries |
| `jq` | any current release | Reading the JSON cursor contract |
| `pytest` | any current release | Unit tests (only required with `--for-tests`) |

## Find an existing install

Claude Code may start with a smaller `PATH` than an interactive shell.  Check
both the command lookup and common user/Homebrew locations:

```bash
command -v sc-compose && sc-compose --version
command -v jq && jq --version
command -v python3 && python3 -c 'import rdflib; print(rdflib.__version__)'
python3 -c 'from importlib.metadata import version; import sc_compose; print(version("sc-compose"))'

for p in "$HOME/.local/bin" "$HOME/.venvs/graph-orchestration/bin" \
  "/opt/homebrew/bin" "/usr/local/bin"; do
  [ -x "$p/sc-compose" ] && echo "sc-compose: $p/sc-compose"
  [ -x "$p/python3" ] && echo "python3: $p/python3"
  [ -x "$p/jq" ] && echo "jq: $p/jq"
done
```

If `rdflib` is installed for a different interpreter, put that interpreter's
directory first for this session, or set `GRAPH_ORCH_PYTHON` to its full path:

```bash
export PATH="/opt/homebrew/bin:$PATH"       # adjust to the path found above
export GRAPH_ORCH_PYTHON="/opt/homebrew/bin/python3"  # optional explicit path
```

Run preflight again after changing `PATH`.  Do not work around a failed check
by invoking `query_runner.py` directly; doing so bypasses the dependency gate.

## Install or upgrade

### macOS

The executable and Python binding are separate artifacts.  Installing the
PyPI wheel does **not** guarantee that a `sc-compose` executable is on `PATH`.
Install both and keep them at the same minimum version.

For the CLI (Homebrew tap):

```bash
brew install randlee/tap/sc-compose
```

For the Python/maturin binding and RDF/test packages (no administrator access
required):

```bash
python3 -m pip install --user --upgrade 'sc-compose>=1.2.0' rdflib pytest
brew install jq
```

If the system `python3` is managed by another tool, use a venv instead:

```bash
python3 -m venv .venv-graph-orchestration
.venv-graph-orchestration/bin/python -m pip install --upgrade pip
.venv-graph-orchestration/bin/python -m pip install 'sc-compose>=1.2.0' rdflib pytest
export PATH="$PWD/.venv-graph-orchestration/bin:$PATH"
brew install randlee/tap/sc-compose
brew install jq
```

### Linux

```bash
python3 -m venv .venv-graph-orchestration
.venv-graph-orchestration/bin/python -m pip install --upgrade pip
.venv-graph-orchestration/bin/python -m pip install 'sc-compose>=1.2.0' rdflib pytest
export PATH="$PWD/.venv-graph-orchestration/bin:$PATH"
sudo apt-get install jq                 # Debian/Ubuntu; use the native package manager otherwise
```

The PyPI package supplies the `sc_compose` Python binding, not the standalone
CLI. Install the CLI from the v1.2.0 release or from a source checkout with
the upstream `cargo install --path crates/sc-compose` procedure, then rerun
preflight.

If distribution Python permits a user install, the equivalent is:

```bash
python3 -m pip install --user --upgrade 'sc-compose>=1.2.0' rdflib pytest
export PATH="$HOME/.local/bin:$PATH"
```

Install the `sc-compose` CLI from the project release channel for the
platform (Homebrew tap, package manager, or a release binary); the pip wheel
above is the Python binding and is not a CLI installation.

Prefer the venv commands when distribution Python refuses user installs
(PEP 668). Do not bypass the protection with `--break-system-packages`.

### Windows (PowerShell)

```powershell
py -m venv .venv-graph-orchestration
.venv-graph-orchestration\Scripts\python -m pip install --upgrade pip
.venv-graph-orchestration\Scripts\python -m pip install 'sc-compose>=1.2.0' rdflib pytest
winget install randlee.sc-compose
winget install jqlang.jq
$env:Path = "$PWD\.venv-graph-orchestration\Scripts;$env:Path"
```

The PyPI package and the standalone Windows CLI are separate artifacts. Restart
the shell if either executable remains invisible.

## Validate after setup

```bash
.claude/skills/graph-orchestration/scripts/preflight --for-tests
python3 -m pytest .claude/skills/graph-orchestration/scripts/test_queries.py -v
python3 -m pytest .claude/skills/graph-orchestration/scripts/test_validate_findings.py -v
```

The first command must return `"success": true` and report
`sc-compose` version `1.2.0` or newer.  A test pass does not override a failed
preflight: dependency errors are distinct from expected validation failures.

## Known issues

- **`sc-compose 1.0.x` is rejected.** Upgrade it; do not lower the requirement.
  Version 1.2.0 supplies the bindings and rendering behavior used by these
  skills.
- **`rdflib` import fails although it was installed.** `pip` and `python3`
  often point at different interpreters.  Compare `command -v python3` with
  `python3 -m pip --version`, then use the same interpreter for installation or
  set `GRAPH_ORCH_PYTHON`.
- **PEP 668 blocks `pip install`.** Use the venv commands above; do not bypass
  the protection with `--break-system-packages` for this workflow.
- **The CLI works in Terminal but not in Claude Code.** Add the discovered
  directory to `PATH` in the current command/session.  Shell startup files are
  not guaranteed to be loaded.
- **`jq` is missing on a minimal machine.** Install it with the platform
  package manager; the graph cursor JSON must not be parsed with ad-hoc text
  tools.
- **`pytest` is missing.** It is a test-time dependency, not a runtime query
  dependency.  Install it or run preflight without `--for-tests` when only
  dispatching a phase.
- **A command returns a malformed version.** The preflight reports a structured
  error and stops.  Check that the command is the real executable rather than
  a shell alias or wrapper emitting extra text.
