set windows-shell := ["pwsh", "-NoLogo", "-Command"]

python_cmd := if os_family() == "windows" { "python" } else { "python3" }
clippy_cmd := if os_family() == "windows" { "cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings" } else { "cargo clippy --workspace --all-targets -- -D warnings" }

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
_lint-unix-gating:
    {{python_cmd}} .just/lint_unix_gating.py

[private]
_lint-runtime-waits:
    {{python_cmd}} .just/lint_runtime_waits.py

[private]
_lint-manifests:
    {{python_cmd}} .just/lint_manifests.py

[private]
_lint-silent-emit:
    {{python_cmd}} scripts/check-silent-emit.py

[private]
_lint-function-length:
    {{python_cmd}} scripts/check-function-length.py

[private]
_lint-legacy-mailbox-paths:
    {{python_cmd}} scripts/check-legacy-mailbox-paths.py

[private]
_lint-capability-degradation:
    {{python_cmd}} scripts/check-capability-degradation.py

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

[private]
_lint-env-var-boundary:
    {{python_cmd}} .just/check_env_var_boundary.py

[private]
_lint-fixed-sleep:
    {{python_cmd}} .just/check_fixed_sleep_hygiene.py

[private]
_lint-ttl-triage:
    {{python_cmd}} .just/lint_ttl_triage_consistency.py

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

[private]
_lint-same-host-portability:
    {{python_cmd}} .just/lint_same_host_portability.py

# Build the full workspace.
build:
    cargo build --workspace
    {{python_cmd}} .just/sign_daemon_dev.py

# Run the full workspace test suite or explicit coverage reporting.
test mode='default':
    {{python_cmd}} .just/run_tests.py {{mode}}

# Validate and plan a bounded adversarial-fuzz campaign (no real execution).
fuzz *args:
    {{python_cmd}} .just/run_fuzz.py {{args}}

# Run the bounded AI.51 campaign against the real local HTTP frame reader.
fuzz-local-http *args:
    {{python_cmd}} scripts/fuzz/run_local_http_framing_campaign.py {{args}}

# Generate or verify the durable public verification-report index.
reports-index *args:
    {{python_cmd}} .just/generate_report_index.py {{args}}

# Build the PyO3 extension with Maturin and prove Python can import it.
test-graft-python:
    {{python_cmd}} scripts/test_atm_graft_python.py

# Build the PyO3 extension and run the Hermes graft reference-adapter tests.
test-hermes-graft-bridge:
    {{python_cmd}} .just/run_hermes_graft_bridge_tests.py

# Compatibility alias for the canonical `just smoke graft-hermes` entry point.
test-hermes-graft-smoke:
    just smoke graft-hermes

# Validate a Hermes bridge registry; append --active only for real operator profiles.
verify-hermes-bridge-deployment profile_registry *args:
    scripts/phase-ai/run-hermes-bridge-probes.sh {{profile_registry}} {{args}}

# Remove workspace build artifacts.
clean:
    cargo clean

# Run the repo lint suite.
lint target='all':
    {{python_cmd}} .just/run_lint.py {{target}}

# Run the retained release validation / preflight suite.
validate target='all':
    {{python_cmd}} scripts/validate_release.py {{target}}

# The sole operator entry point for smoke testing. Every Python smoke module is
# internal to this recipe; use `just smoke <feature>` instead of invoking one.
# `localhost` proves an ordinary self-send through the advertised physical
# interface; `local-ip` then adds IPv4 loopback. Cross-host stages use public
# ATM clients against already-running peer daemons.
smoke feature='normal' *args:
    if [ "{{feature}}" = "peer-pair" ]; then {{python_cmd}} scripts/smoke/run_peer_pair.py {{args}}; elif [ "{{feature}}" = "inbound-peer" ]; then {{python_cmd}} scripts/smoke/run_inbound_peer_smoke.py {{args}}; elif [ "{{feature}}" = "inbound-peer-combine" ]; then {{python_cmd}} scripts/smoke/combine_inbound_peer_smoke.py {{args}}; elif [ "{{feature}}" = "graft-hermes" ]; then {{python_cmd}} scripts/phase-ai/run-hermes-graft-smoke.py {{args}}; else {{python_cmd}} scripts/smoke/run_feature_smoke.py {{feature}} {{args}}; fi

# Run one isolated, release-built local admission benchmark. On Unix choose
# UDS or loopback TCP; Windows accepts TCP only. The runner rejects ambient
# daemon/database state and writes one report-compatible JSON artifact per run.
benchmark *args:
    cargo build --release -p agent-team-mail -p atm-daemon
    {{python_cmd}} .just/sign_daemon_dev.py
    {{python_cmd}} scripts/smoke/run_admission_capacity.py {{args}}

# Persist AI.40 benchmark JSON and render the aggregate public report.
benchmark-report *args:
    {{python_cmd}} scripts/smoke/benchmark_report.py {{args}}

# Generate architecture visualization artifacts.
view target='all':
    {{python_cmd}} .just/run_view.py {{target}}

# Run the local CI-equivalent command set.
ci: lint test
