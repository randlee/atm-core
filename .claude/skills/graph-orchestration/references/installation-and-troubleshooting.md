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
| `sc-compose` CLI | **released v1.6.1 prebuilt release binary** | Rendering fenced Jinja templates and agent prompts |
| `sc_compose` Python binding | **1.6.1** | Python/maturin rendering integrations and wrappers |
| Python + `rdflib` | **3.11+** | RDF/Turtle parsing and SPARQL queries |
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

for p in "$HOME/.local/bin" \
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

For the CLI, use the pinned prebuilt release asset required by the report templates:

```bash
Download the platform-matching v1.6.1 archive from `randlee/sc-compose`,
verify its SHA256 against `checksums.txt`, and unpack `sc-compose` into the
bootstrap tools directory. `just bootstrap` performs this hard-fail check;
never compile the CLI from source.
```

For the Python/maturin binding (a one-time per-machine setup; no activation
step):

```bash
python3 -m pip install --user --break-system-packages 'sc-compose==1.6.1'
brew install jq
```

The `--user` target keeps the wheel out of Homebrew-managed site-packages;
`--break-system-packages` is the explicit Python override for this trusted,
user-owned install. The wheel is published on PyPI and provides the
`sc_compose` binding (Python 3.11+; prebuilt platform wheels). It does not install the standalone CLI, so keep the
source-pinned CLI install above as a separate step. This is done once per machine;
callers only perform a guarded `import sc_compose` on each invocation.

### Linux

```bash
python3 -m pip install --user --break-system-packages 'sc-compose==1.6.1'
sudo apt-get install jq                 # Debian/Ubuntu; use the native package manager otherwise
```

The PyPI package supplies the `sc_compose` Python binding, not the standalone
CLI. Install the prebuilt CLI from the pinned release above, then rerun
preflight.

The pip wheel above is the Python binding and is not a CLI installation; use
the source-pinned CLI command above on every platform.
If the invoking Python is not `python3`, substitute that interpreter in the
one-time install command and in the preflight.

### Windows (PowerShell)

```powershell
py -m pip install --user --break-system-packages "sc-compose==1.6.1"
Download and verify the platform-matching v1.6.1 release archive as described
above; do not compile the CLI from source.
winget install jqlang.jq
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
the pinned released v1.6.1 `sc-compose` build. A test pass does not override a failed
preflight: dependency errors are distinct from expected validation failures.

## Known issues

- **The v1.3.0 CLI is rejected for current templates.** Install the pinned
  v1.6.1 prebuilt release above; the Python binding remains a separate `>=1.6.1`
  artifact.
- **`rdflib` import fails although it was installed.** `pip` and `python3`
  often point at different interpreters.  Compare `command -v python3` with
  `python3 -m pip --version`, then use the same interpreter for installation or
  set `GRAPH_ORCH_PYTHON`.
- **PEP 668 blocks `pip install`.** Run the sanctioned one-time user install:
  `python3 -m pip install --user --break-system-packages 'sc-compose==1.6.1'`.
  No venv or activation step is required.  If that interpreter is not the one
  used by the skill, repeat the command with the exact invoking interpreter.
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
