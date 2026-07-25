# Triaging-findings dependency setup

This reference is loaded only when the Step 1 preflight fails. The skill
requires:

| Dependency | Minimum | Purpose |
|---|---:|---|
| `sc-compose` CLI | **1.2.0** | Render canonical Turtle records |
| Python `sc_compose` binding | **1.2.0** | Native Python rendering/API tests |
| `oxigraph` | supported installed release | Parse/validate rendered Turtle |
| `rg` | installed | Sweep source worktrees |
| Python `rdflib` | installed in the invoking Python | Graph/query support |

## Check first

From the repository root, run:

```bash
python3 .claude/skills/triaging-findings/scripts/check_dependencies.py
```

The checker searches PATH and common Homebrew, Cargo, user-bin, and Windows
locations. If it reports success, do not reinstall anything.

## Install

macOS (recommended, avoids PEP 668 system-Python restrictions):

```bash
brew install randlee/tap/sc-compose
python3 -m venv .venv-triaging-findings
.venv-triaging-findings/bin/python -m pip install --upgrade pip
.venv-triaging-findings/bin/python -m pip install 'sc-compose>=1.2.0' rdflib pytest
export PATH="$PWD/.venv-triaging-findings/bin:$PATH"
cargo install oxigraph-cli
```

The PyPI package supplies the `sc_compose` Python binding; it does not replace
the standalone `sc-compose` CLI. On Linux without Homebrew, install the CLI
from the v1.2.0 release or from a source checkout with the upstream
`cargo install --path crates/sc-compose` procedure.

Linux:

```bash
python3 -m venv .venv-triaging-findings
.venv-triaging-findings/bin/python -m pip install --upgrade pip
.venv-triaging-findings/bin/python -m pip install 'sc-compose>=1.2.0' rdflib pytest
export PATH="$PWD/.venv-triaging-findings/bin:$PATH"
# Install the standalone CLI from the v1.2.0 release, or from source:
# cargo install --path crates/sc-compose
cargo install oxigraph-cli
```

Install `ripgrep` with the platform package manager if `rg` is absent:

```bash
brew install ripgrep                 # macOS
sudo apt-get install ripgrep         # Debian/Ubuntu
```

Windows PowerShell:

```powershell
py -m venv .venv-triaging-findings
.venv-triaging-findings\Scripts\python -m pip install --upgrade pip
.venv-triaging-findings\Scripts\python -m pip install "sc-compose>=1.2.0" rdflib pytest
cargo install oxigraph-cli
winget install BurntSushi.ripgrep.MSVC
```

`winget install randlee.sc-compose` installs the standalone Windows CLI; the
PyPI install above supplies the Python binding.

Install into the same interpreter used to invoke `check_dependencies.py`.

## PATH troubleshooting

Claude Code may start with a smaller PATH than an interactive shell. Locate
the binaries explicitly:

```bash
for name in sc-compose oxigraph rg; do
  command -v "$name" || true
done
printf '%s\n' "$PATH"
```

Common fixes:

```bash
export PATH="/opt/homebrew/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
```

Alternatively invoke the absolute path printed by the checker. Do not place a
host-specific absolute checkout path in a canonical Turtle record.

## Validate after setup

```bash
sc-compose --version       # CLI must be 1.2.0 or newer
oxigraph --version
rg --version
python3 -c 'import rdflib; print(rdflib.__version__)'
python3 -c 'import sc_compose, importlib.metadata as m; print(m.version("sc-compose"))'
python3 .claude/skills/triaging-findings/scripts/check_dependencies.py
```

## Known issues

- `sc-compose 1.0.x` is insufficient; upgrade rather than suppressing the
  version failure.
- Installing with one Python and invoking the skill with another leaves
  `rdflib` unavailable. Compare `python3 -c 'import sys; print(sys.executable)'`
  with the interpreter used for installation.
- Homebrew/system Python may reject global `pip install` with an
  `externally-managed-environment` (PEP 668) error. Use the venv commands above;
  do not bypass the protection with `--break-system-packages` for this workflow.
- `cargo install` puts `oxigraph` under `~/.cargo/bin`; ensure that directory is
  visible to the Claude Code shell.
- If a dependency is unavailable, stop the triage workflow. Do not write a
  partial `.ttl` record or dispatch a fix assignment.
