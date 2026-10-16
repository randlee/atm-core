---
name: sc-lint-adoption
description: Install or update the managed sc-lint contract in a Rust repository. Use when a repository needs unified `just setup`, `just lint`, and `just test` commands, or when reviewing adoption drift.
version: 1.0.0
---

# sc-lint adoption

Use this skill only in the consumer repository being changed. The adoption kit
renders its managed files; it never deletes consumer files. Keep the consumer's
existing commands and configuration unless the rendered contract replaces that
same responsibility.

The kit root below is the installed `sc-lint-adoption` package. Set it once for
the session to its actual installation directory; do not copy its Python code
into the consumer repository.

```bash
export SC_LINT_ADOPTION_ROOT="${CLAUDE_PLUGIN_ROOT:?Claude Code must provide the installed sc-lint-adoption package root}"
```

## 1. Collect facts

Record the following facts in the consumer PR body before planning: workspace
members, current root `Justfile` recipes, CI operating-system matrix, and active
Rust toolchain. Each command is safe when the corresponding optional surface is
absent.

```bash
cargo metadata --no-deps --format-version 1
if [ -f Justfile ]; then just --list; fi
if [ -f Justfile ]; then rg -n '^test-[[:alnum:]_-]+:' Justfile || true; fi
if [ -d .github/workflows ]; then grep -RIn "runs-on:" .github/workflows; fi
rustup show active-toolchain
```

Use those facts to decide analyzer `enabled` and `reason` fields: set runtime
analysis only when the repository uses an async runtime, and scope portability
targets to the observed CI platforms. Do not invent a registry of analyzers.

### Migrate named test recipes

For every existing root `test-<name>` recipe, preserve its underlying command
as the consumer-owned `test.<name>` entry in `install.json`; the installer
renders that entry as a `[[tool.sc-lint.test]]` layer. Replace that recipe's
body with `just test <name>` so its existing name remains a thin compatibility
alias for the declared layer. Keep all consumer-owned recipes that are not
tests unchanged. This is a scoped consumer-PR migration; the kit does not
guess commands, scan arbitrary scripts, or delete recipes.

### Resolve recipe and setting collisions

The kit defines `setup`, `lint` and `upgrade`, and no `default`, `test` or `set`.
Rename or remove any root `lint`, `setup` or `upgrade` recipe, moving its command
into `profiles.<name>`. The repository owns `just test`: a root `test` recipe
is kept as is; if there is none, the installer adds sc-lint's recommended one
(`plugins/sc-lint/templates/test.just`) outside the managed block. Keep a consumer `default` recipe;
none is added to an existing Justfile. If the root `Justfile` sets `windows-shell`, the kit does not add one;
set `"justfile": {"windows_shell": false}` to omit it entirely. Never duplicate a `set`.

## 2. Write and validate `install.json`

Create `install.json` at the consumer root. It has `minimum_version`,
`profiles`, `ci`, `analyzers`, and optional `test` and `justfile` fields.
`justfile.windows_shell` is a boolean that defaults to `true`; set it to
`false` to omit the kit's `set windows-shell` line. The dry-run in step 3
validates this input against the packaged `install.schema.json` before it
renders any file.

```bash
python3 -m json.tool install.json >/dev/null
python3 -c 'import json; json.load(open("install.json")); print("install.json: valid JSON")'
```

Kit-rendered command arrays must name a shipped binary or `sc_lint` module only;
profile and test-layer commands are consumer-owned and rendered verbatim. A
repository-relative script is not authorized in a kit-rendered step.

## 3. Run the drift check

Run the installer without writes first and retain both its output and exit code
in the PR body. Exit 0 means already converged; exit 1 means a safe proposed
change; exit 2 means an input, managed-marker, pre-existing-file or Justfile
recipe/setting collision conflict that must be resolved. On every install,
differing pre-existing kit targets absent from the prior installed manifest are
listed, and nothing is written. The merged root `Justfile` and `.sc-lint/install.json` when selected as the input
are exempt from that clash check. When CI is enabled, the generated workflow
uses `./plugins/sc-lint/.github/actions/setup-sc-lint`, vendored at
`plugins/sc-lint/.github/actions/setup-sc-lint/action.yml`.

```bash
set +e; python3 "$SC_LINT_ADOPTION_ROOT/install.py" --dry-run --input install.json .; dry_run_exit=$?; set -e; printf 'sc-lint adoption dry-run exit %s\n' "$dry_run_exit"; test "$dry_run_exit" -le 1
```

Continue only for exit 0 or 1. Do not override exit 2 or manually edit a
managed marker block.

## 4. Install the managed contract

After reviewing the diff, apply exactly the same input.

```bash
python3 "$SC_LINT_ADOPTION_ROOT/install.py" --input install.json .
```

## Offline documentation

After installation, locate the product-owned offline documentation bundle with
`sc-lint docs --path`. Use `sc-lint docs` for the installed overview and
`sc-lint docs <guide>` for a named guide such as `installation` or
`troubleshooting`. These commands use the installed product bundle; do not
look for documentation in a source checkout.

## 5. Verify the consumer interface

Run the aggregate commands exactly. They are the only routine command contract
an agent needs after adoption.

```bash
just setup && just lint && just test
```

## 6. Remove superseded consumer-local scaffolding in the consumer PR

Only after the commands pass, identify consumer-local sc-lint wrappers,
duplicated installer logic, and redundant CI snippets now provided by the kit.
List every removal and the reason in the consumer PR body. The kit itself never
deletes these files; the adopting agent makes the scoped consumer-PR change.

## 7. Open the consumer PR with evidence

Open one consumer PR. Include collected facts, the `install.json` summary,
managed-file diff, the literal `sc-lint adoption dry-run exit 0` or exit-1
result, aggregate-command output, and the removal list. If any post-install
drift remains, include the new dry-run output instead of claiming convergence.

## How to extend

- **Analyzers:** set `analyzers.<name>` in `install.json`; the kit renders it as
  `[tool.sc-lint.analyzers.<name>]`. The keys are listed in `configuration.md`,
  "Analyzer tables"; any other key fails the install.
- **Test layers:** declare ordered `[tool.sc-lint.test.<layer>]` lists. `unit`
  is the default; `just test <layer> *args` passes through and `just test all`
  follows declared order.
- **Lint profiles:** declare ordered `[tool.sc-lint.lint.<profile>]` steps.
- **Consumer-owned recipes:** add non-managed recipes outside the managed
  `Justfile` import block. Do not alter the marker block.

See `tests/fixtures/adoption/analyzer-worked-example/` for a complete worked
configuration showing analyzer reasons, test layers, profiles, and a
consumer-owned recipe.

`adopt.xml.j2` is the ATM-rendered form of these same seven steps: the
orchestrator renders it with the repository, `install.json`, and completion
recipient, then the `sc-lint-adopter` agent consumes that assignment. It is
not a second adoption procedure.
