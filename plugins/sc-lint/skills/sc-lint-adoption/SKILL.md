---
name: sc-lint-adoption
description: Seed the sc-lint contract in a Rust repository. Use when a repository needs unified `just setup`, `just lint`, and `just test` commands, or when reviewing adoption inputs.
version: 0.8.8
---

# sc-lint adoption

Use this skill only in the consumer repository being changed.
The kit root below is the installed `sc-lint-adoption` package. Set it once for
the session to its actual installation directory; do not copy its Python code
into the consumer repository.

```bash
export SC_LINT_ADOPTION_ROOT="${CLAUDE_PLUGIN_ROOT:-plugins/sc-lint}"
test -f "$SC_LINT_ADOPTION_ROOT/install.py" || { echo "sc-lint-adoption root not found: set CLAUDE_PLUGIN_ROOT to the installed package or vendor the kit at plugins/sc-lint" >&2; exit 1; }
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

## 2. Write and validate `install.json`

Create `install.json` at the consumer root. It has `minimum_version`,
`profiles`, `ci`, `analyzers`, and optional `test` and `justfile` fields. It may
also set `lint_profiles`, `tools`, `default_lint_profile`, `line_counts`,
`identities`, `version_sync`, `cargo_shear`, `logging`, and `artifacts`.
`justfile.windows_shell` is a boolean that defaults to `true`; set it to
`false` to omit the kit's `set windows-shell` line. The dry-run in step 3
validates this input against the packaged `install.schema.json` before it
renders any file.

```bash
python3 -m json.tool install.json >/dev/null
python3 -c 'import json; json.load(open("install.json")); print("install.json: valid JSON")'
```

## 3. Preview missing kit files

The installer seeds only missing destinations. Existing files, directories,
and symlinks are left untouched regardless of their contents; the installer
does not merge or refresh them. Run it without writes first and retain both its output and exit code
in the PR body. Exit 0 means no destinations are missing; exit 1 means the
output lists destinations that would be seeded; exit 2 means the input is
invalid. When CI is enabled, the generated workflow
uses `./plugins/sc-lint/.github/actions/setup-sc-lint`, vendored at
`plugins/sc-lint/.github/actions/setup-sc-lint/action.yml`. The sc-lint GitHub
Action (`action.yml` at the sc-lint repository root) is an alternative that
installs a verified release and runs the same lint or test contract; see
`docs/sc-lint/github-action-requirements.md` in the sc-lint repository. Do not
mix both in one workflow without a reason recorded in the consumer PR.

```bash
set +e; python3 "$SC_LINT_ADOPTION_ROOT/install.py" --dry-run --input install.json .; dry_run_exit=$?; set -e; printf 'sc-lint adoption dry-run exit %s\n' "$dry_run_exit"; test "$dry_run_exit" -le 1
```

Continue only for exit 0 or 1.

## 4. Seed missing destinations

After reviewing the preview, use exactly the same input to seed missing
destinations. The installer does not update existing consumer paths.

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
seeded-path diff, the literal `sc-lint adoption dry-run exit 0` or exit-1
result, aggregate-command output, and the removal list. The dry-run reports
missing destinations, not drift in existing consumer files.

## How to extend

- **Analyzers:** set `analyzers.<name>` in `install.json`; the kit renders it as
  `[tool.sc-lint.analyzers.<name>]`. The keys are listed in `configuration.md`,
  "Analyzer tables"; any other key fails the install.
- **Test layers:** each `test` entry renders one `[[tool.sc-lint.test]]` row with
  a `name`. `just test` with no selector runs every row in declared order;
  `just test <name> *args` runs one row. No layer name is special.
- **Lint steps and profiles:** each lint step renders one `[[tool.sc-lint.lint]]`
  row with a `name`. `lint_profiles` renders the `[tool.sc-lint.lint_profiles]`
  table, which maps a profile name to an ordered list of step names.

See `tests/fixtures/adoption/analyzer-worked-example/` for a complete worked
configuration showing analyzer reasons, test layers, profiles, and a
consumer-owned recipe.

`adopt.xml.j2` is the ATM-rendered form of these same seven steps: the
orchestrator renders it with the repository, `install.json`, and completion
recipient, then the `sc-lint-adopter` agent consumes that assignment. It is
not a second adoption procedure.
