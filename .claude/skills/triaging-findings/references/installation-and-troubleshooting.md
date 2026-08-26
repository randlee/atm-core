# Triaging-findings dependency setup

This reference is loaded only when the Step 1 preflight fails. The skill
requires:

| Dependency | Minimum | Purpose |
|---|---:|---|
| `sc-compose` CLI | **released v1.5.0 prebuilt release binary** | Render canonical Turtle records |
| Python `sc_compose` binding | **1.2.0** | Native Python rendering/API tests |
| `oxigraph` | supported installed release | Parse/validate rendered Turtle |
| `rg` | installed | Sweep source worktrees |
| Python `rdflib` | **3.11+**, installed in the invoking Python | Graph/query support |

## Check first

From the repository root, run:

```bash
python3 .claude/skills/triaging-findings/scripts/check_dependencies.py
```

The checker searches PATH and common Homebrew, Cargo, user-bin, and Windows
locations. If it reports success, do not reinstall anything.

## Install

macOS (one-time per-machine setup):

```bash
python3 -m pip install --user --break-system-packages 'sc-compose>=1.2.0'
# The CLI pin below is required for the current report/template contract.
Download the platform-matching `sc-compose` v1.5.0 release archive from
`randlee/sc-compose`, verify its SHA256 against the release `checksums.txt`,
and unpack the executable into the bootstrap tools directory. `just bootstrap`
performs this verification automatically; do not compile it with Cargo.
cargo install oxigraph-cli
```

The `--user` target avoids Homebrew-managed site-packages and
`--break-system-packages` is Python's sanctioned override for this trusted
user-owned wheel. Do not create or activate a venv for this setup. The PyPI
package supplies the `sc_compose` Python binding (Python 3.11+; prebuilt
platform wheels); it does not replace the standalone `sc-compose` CLI. On
Linux without Homebrew, install the CLI from the pinned release
shown above.

Linux:

```bash
python3 -m pip install --user --break-system-packages 'sc-compose>=1.2.0'
# Install the pinned released standalone CLI:
Download and verify the platform-matching v1.5.0 release archive as described
above; do not compile the CLI with Cargo.
cargo install oxigraph-cli
```

Install `ripgrep` with the platform package manager if `rg` is absent:

```bash
brew install ripgrep                 # macOS
sudo apt-get install ripgrep         # Debian/Ubuntu
```

Windows PowerShell:

```powershell
py -m pip install --user --break-system-packages "sc-compose>=1.2.0"
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
sc-compose --version       # CLI must be the pinned released v1.5.0 build
oxigraph --version
rg --version
python3 -c 'import rdflib; print(rdflib.__version__)'
python3 -c 'import sc_compose, importlib.metadata as m; print(m.version("sc-compose"))'
python3 .claude/skills/triaging-findings/scripts/check_dependencies.py
```

## Known issues

- The v1.3.0 CLI is insufficient for the current templates. Install the pinned
  v1.5.0 prebuilt release above; do not lower the requirement or substitute an older release
  binary.
- Installing with one Python and invoking the skill with another leaves
  `rdflib` unavailable. Compare `python3 -c 'import sys; print(sys.executable)'`
  with the interpreter used for installation.
- Homebrew/system Python may reject plain `pip install` with an
  `externally-managed-environment` (PEP 668) error. Use the sanctioned,
  one-time user install `python3 -m pip install --user
  --break-system-packages 'sc-compose>=1.2.0'`; no venv or activation step is
  required.
- `cargo install` puts `oxigraph` under `~/.cargo/bin`; ensure that directory is
  visible to the Claude Code shell.
- If a dependency is unavailable, stop the triage workflow. Do not write a
  partial `.ttl` record or dispatch a fix assignment.
