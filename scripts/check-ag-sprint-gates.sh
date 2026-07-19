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

info "running delete-list denylist gate"
cargo test -p atm-architecture --test boundary_enforcement \
  ag_delete_lists_must_have_no_forbidden_symbols_or_workaround_paths

info "running active sprint diff gate for sprint=$SPRINT base=$BASE head=$HEAD_REF"
ATM_ARCH_ACTIVE_SPRINT="$SPRINT" \
ATM_ARCH_DIFF_BASE="$BASE" \
ATM_ARCH_DIFF_HEAD="$HEAD_REF" \
ATM_ARCH_ALLOW_GATE_CHANGES="$ALLOW_GATE_CHANGES" \
cargo test -p atm-architecture --test boundary_enforcement \
  active_sprint_diff_gate_must_hold_when_configured

info "PASS - AG sprint gate checks satisfied"
