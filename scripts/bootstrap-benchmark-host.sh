#!/usr/bin/env bash
# Provision one macOS benchmark-host checkout from the repository's exact pins.
#
# This is intentionally a host-tool bootstrap only: it neither starts a daemon
# nor runs a benchmark.  `just benchmark` remains an explicit, separately
# authorized operation for the dedicated benchmark account.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$root/tools/bootstrap.toml"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "bootstrap refused: benchmark-host bootstrap currently supports macOS only" >&2
    exit 1
fi

if [[ ! -f "$manifest" ]]; then
    echo "bootstrap refused: exact tool manifest is missing: $manifest" >&2
    exit 1
fi

python_version="$(awk -F '"' '/^python = / { print $2; exit }' "$manifest")"
rust_version="$(awk -F '"' '/^rust = / { print $2; exit }' "$manifest")"
just_version="$(awk -F '"' '/^just = / { print $2; exit }' "$manifest")"

if [[ ! "$python_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! "$rust_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ || ! "$just_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "bootstrap refused: tools/bootstrap.toml must contain exact Python, Rust, and Just versions" >&2
    exit 1
fi

python_minor="${python_version%.*}"
brew=""
for candidate in /opt/homebrew/bin/brew /usr/local/bin/brew; do
    if [[ -x "$candidate" ]]; then
        brew="$candidate"
        break
    fi
done
if [[ -z "$brew" ]]; then
    echo "bootstrap refused: Homebrew is required at /opt/homebrew/bin/brew or /usr/local/bin/brew" >&2
    exit 1
fi
if ! command -v rustup >/dev/null; then
    echo "bootstrap refused: rustup is required to install Rust $rust_version" >&2
    exit 1
fi

python_prefix="$("$brew" --prefix "python@$python_minor")"
seed_python="$python_prefix/bin/python$python_minor"
brew_just="$("$brew" --prefix just)/bin/just"
python_matches=false
just_matches=false
if [[ -x "$seed_python" && "$("$seed_python" --version 2>&1)" == "Python $python_version" ]]; then
    python_matches=true
fi
if [[ -x "$brew_just" && "$("$brew_just" --version)" == "just $just_version" ]]; then
    just_matches=true
fi

# A shared Homebrew cellar may intentionally be read-only to this benchmark
# account. Do not ask it to mutate packages that already meet the exact
# contract; a mismatch remains an explicit repair operation.
if [[ "$python_matches" != true || "$just_matches" != true ]]; then
    "$brew" install "python@$python_minor" just
    "$brew" upgrade "python@$python_minor" just
    python_prefix="$("$brew" --prefix "python@$python_minor")"
    seed_python="$python_prefix/bin/python$python_minor"
    brew_just="$("$brew" --prefix just)/bin/just"
fi
if [[ ! -x "$seed_python" || "$("$seed_python" --version 2>&1)" != "Python $python_version" ]]; then
    echo "bootstrap refused: Homebrew Python does not match exact pin $python_version" >&2
    exit 1
fi
if [[ ! -x "$brew_just" || "$("$brew_just" --version)" != "just $just_version" ]]; then
    echo "bootstrap refused: Homebrew Just does not match exact pin $just_version" >&2
    exit 1
fi

rustup toolchain install "$rust_version" --profile minimal --component clippy,rustfmt
cd "$root"
rustup override set "$rust_version"
"$brew_just" bootstrap

# Build with the isolated venv selected so PyO3 cannot discover macOS's system
# Python.  The recipe signs the CLI and daemon only when a development identity
# is present; it does not launch either binary.
PATH="$root/.bootstrap-venv/bin:$PATH" \
PYO3_PYTHON="$root/.bootstrap-venv/bin/python" \
"$brew_just" build

echo "benchmark host bootstrap complete: exact pinned toolchain and signed build verified."
