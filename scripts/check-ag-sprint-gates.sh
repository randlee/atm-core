#!/usr/bin/env bash
set -euo pipefail

fail() {
  echo "ag-sprint-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "ag-sprint-gate: $*"
}

usage() {
  cat <<'EOF'
Usage:
  scripts/check-ag-sprint-gates.sh <SPRINT> <BASE> <HEAD> [--allow-gate-changes]

Examples:
  scripts/check-ag-sprint-gates.sh AG.18 origin/develop HEAD
  scripts/check-ag-sprint-gates.sh AG.21 50572166 HEAD

Behavior:
  1. Runs the global AG delete-list denylist gate
  2. Runs the active sprint diff/LOC gate for the supplied sprint/base/head

Environment:
  ATM_ARCH_ALLOW_GATE_CHANGES=1 may be used instead of --allow-gate-changes,
  but only for dedicated architecture-gate maintenance work.
EOF
}

[[ $# -ge 3 ]] || {
  usage
  fail "expected at least 3 arguments"
}

SPRINT="$1"
BASE="$2"
HEAD_REF="$3"
shift 3

ALLOW_GATE_CHANGES="${ATM_ARCH_ALLOW_GATE_CHANGES:-0}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --allow-gate-changes)
      ALLOW_GATE_CHANGES=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

git rev-parse --verify "$BASE" >/dev/null 2>&1 || fail "missing base ref: $BASE"
git rev-parse --verify "$HEAD_REF" >/dev/null 2>&1 || fail "missing head ref: $HEAD_REF"

manifest_path="crates/atm-architecture/delete-lists/$(tr '[:upper:]' '[:lower:]' <<<"$SPRINT" | tr -d '.').toml"
boundary_test_path="crates/atm-architecture/tests/boundary_enforcement.rs"
gate_base="$BASE"

# A deletion sprint may introduce or strengthen its own immutable manifest, but
# that work must be a dedicated bootstrap commit before the gated product diff.
# Treating it as ordinary sprint work would let a PR weaken its own guard after
# changing product code.  The bootstrap commit may touch only the active
# manifest and the enforcement test; later changes to either file fail closed.
gate_seed_commits=()
while IFS= read -r commit; do
  [[ -n "$commit" ]] && gate_seed_commits+=("$commit")
done < <(git rev-list --reverse "$BASE..$HEAD_REF" -- "$manifest_path" "$boundary_test_path")
if [[ ${#gate_seed_commits[@]} -gt 0 ]]; then
  gate_seed="${gate_seed_commits[0]}"
  seed_files=()
  while IFS= read -r file; do
    [[ -n "$file" ]] && seed_files+=("$file")
  done < <(git diff-tree --no-commit-id --name-only -r "$gate_seed")
  for file in "${seed_files[@]}"; do
    case "$file" in
      "$manifest_path"|"$boundary_test_path") ;;
      *) fail "$SPRINT: gate bootstrap commit $gate_seed changed non-gate file $file" ;;
    esac
  done
  [[ ${#gate_seed_commits[@]} -eq 1 ]] || fail "$SPRINT: gate files changed after bootstrap commit $gate_seed"
  gate_base="$gate_seed"
  info "using dedicated gate bootstrap $gate_seed; product diff begins after it"
fi

info "running delete-list denylist gate"
ATM_ARCH_ACTIVE_SPRINT="$SPRINT" \
cargo test -p atm-architecture --test boundary_enforcement \
  ag_delete_lists_must_have_no_forbidden_symbols_or_workaround_paths

info "running active sprint diff gate for sprint=$SPRINT base=$gate_base head=$HEAD_REF"
ATM_ARCH_ACTIVE_SPRINT="$SPRINT" \
ATM_ARCH_DIFF_BASE="$gate_base" \
ATM_ARCH_DIFF_HEAD="$HEAD_REF" \
ATM_ARCH_ALLOW_GATE_CHANGES="$ALLOW_GATE_CHANGES" \
cargo test -p atm-architecture --test boundary_enforcement \
  active_sprint_diff_gate_must_hold_when_configured

info "PASS - AG sprint gate checks satisfied"
