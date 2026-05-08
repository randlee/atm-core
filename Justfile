set windows-shell := ["pwsh", "-NoLogo", "-Command"]

python_cmd := if os_family() == "windows" { "python" } else { "python3" }
windows_clippy_scope := env_var_or_default("ATM_WINDOWS_CLIPPY_SCOPE", "cross-platform-only")
clippy_cmd := if os_family() == "windows" { if windows_clippy_scope == "cross-platform-only" { "cargo clippy --workspace --all-targets --exclude atm-daemon --target x86_64-pc-windows-msvc -- -D warnings" } else { "cargo clippy --workspace --all-targets -- -D warnings" } } else { "cargo clippy --workspace --all-targets -- -D warnings" }

# Show the curated repo task help.
default: help

# Show the curated repo task help.
help:
    {{python_cmd}} .just/print_help.py

[private]
_fmt-write:
    cargo fmt --all

[private]
_fmt-check:
    cargo fmt --all --check

# Format the Rust workspace or run the formatting gate.
fmt mode='check':
    {{python_cmd}} .just/run_fmt.py {{mode}}

[private]
_lint-fmt:
    @just fmt check

[private]
_lint-clippy:
    {{clippy_cmd}}

[private]
_lint-modules:
    {{python_cmd}} .just/lint_cargo_modules.py

[private]
_lint-deny:
    {{python_cmd}} .just/lint_cargo_deny.py

[private]
_lint-shear:
    {{python_cmd}} .just/lint_cargo_shear.py

[private]
_lint-boundaries:
    {{python_cmd}} .just/lint_boundaries.py

[private]
_lint-sc-boundary:
    {{python_cmd}} .just/lint_sc_boundary.py

[private]
_lint-sc-portability:
    {{python_cmd}} .just/lint_sc_portability.py

[private]
_lint-manifests:
    {{python_cmd}} .just/lint_manifests.py

# Verify crate/release versions stay aligned.
[private]
_lint-version:
    {{python_cmd}} .just/check_version_sync.py

# Show current workspace version state or latest recommended direct dependency upgrades.
version mode='current':
    {{python_cmd}} .just/run_version.py {{mode}}

# Enforce RULE-008 for test and cfg(test) code.
[private]
_lint-identities:
    {{python_cmd}} .just/check_test_identity_literals.py

# Enforce RULE-003 source file size limits.
[private]
_lint-lines:
    {{python_cmd}} .just/check_line_counts.py

[private]
_lint-spell:
    {{python_cmd}} .just/lint_codespell.py

[private]
_lint-pytests:
    {{python_cmd}} .just/run_pytests.py

[private]
_lint-daemon-singleton:
    {{python_cmd}} scripts/lint_daemon_singleton.py

# Build the full workspace.
build:
    cargo build --workspace

# Run the full workspace test suite.
test:
    cargo build --workspace
    cargo test --workspace

# Remove workspace build artifacts.
clean:
    cargo clean

# Run the repo lint suite.
lint target='all':
    {{python_cmd}} .just/run_lint.py {{target}}

# Generate architecture visualization artifacts.
view target='all':
    {{python_cmd}} .just/run_view.py {{target}}

# Run the local CI-equivalent command set.
ci: lint test
