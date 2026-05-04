set windows-shell := ["pwsh", "-NoLogo", "-Command"]

python_cmd := if os_family() == "windows" { "python" } else { "python3" }

# Show the curated repo task help.
default: help

# Show the curated repo task help.
help:
    {{python_cmd}} .just/print_help.py

# Format the Rust workspace in place.
fmt:
    cargo fmt --all

# Check Rust formatting.
fmt-check:
    cargo fmt --all --check

# Run Clippy with warnings denied.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Verify crate/release versions stay aligned.
version-check:
    {{python_cmd}} .just/check_version_sync.py

# Enforce RULE-008 for test and cfg(test) code.
lint-identities:
    {{python_cmd}} .just/check_test_identity_literals.py

# Enforce RULE-003 source file size limits.
lint-lines:
    {{python_cmd}} .just/check_line_counts.py

# Build the full workspace.
build:
    cargo build --workspace

# Run the full workspace test suite.
test:
    cargo test --workspace

# Remove workspace build artifacts.
clean:
    cargo clean

# Run the repo lint suite.
lint: fmt-check clippy version-check lint-identities lint-lines

# Run the local CI-equivalent command set.
ci: lint test
