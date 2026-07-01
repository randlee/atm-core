#!/usr/bin/env bash
set -euo pipefail

MAIN_REF="${1:-origin/main}"
DEVELOP_REF="${2:-origin/develop}"
TRIGGER_REF="${3:-${GITHUB_REF:-}}"

fail() {
  echo "release-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "release-gate: $*"
}

normalize_trigger_ref() {
  if [[ -n "$TRIGGER_REF" ]]; then
    printf '%s\n' "$TRIGGER_REF"
    return 0
  fi

  local current_branch
  current_branch="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
  if [[ -z "$current_branch" || "$current_branch" == "HEAD" ]]; then
    return 1
  fi

  printf 'refs/heads/%s\n' "$current_branch"
}

info "fetching refs and tags"
git fetch origin --prune --tags >/dev/null 2>&1 || fail "git fetch failed"

git rev-parse --verify "$MAIN_REF" >/dev/null 2>&1 || fail "missing ref: $MAIN_REF"
git rev-parse --verify "$DEVELOP_REF" >/dev/null 2>&1 || fail "missing ref: $DEVELOP_REF"

main_sha="$(git rev-parse "$MAIN_REF")"
develop_sha="$(git rev-parse "$DEVELOP_REF")"
info "main=$main_sha develop=$develop_sha"

trigger_ref="$(normalize_trigger_ref)" || fail "unable to determine triggering branch ref"
[[ "$trigger_ref" =~ ^refs/heads/release/v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail \
  "triggering ref must match refs/heads/release/vX.Y.Z (got: $trigger_ref)"
info "trigger_ref=$trigger_ref"

info "PASS - release gate checks satisfied"
