#!/usr/bin/env bash
set -euo pipefail

MAIN_REF="${1:-origin/main}"
DEVELOP_REF="${2:-origin/develop}"

fail() {
  echo "release-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "release-gate: $*"
}

info "fetching refs and tags"
git fetch origin --prune --tags >/dev/null 2>&1 || fail "git fetch failed"

git rev-parse --verify "$MAIN_REF" >/dev/null 2>&1 || fail "missing ref: $MAIN_REF"
git rev-parse --verify "$DEVELOP_REF" >/dev/null 2>&1 || fail "missing ref: $DEVELOP_REF"

main_sha="$(git rev-parse "$MAIN_REF")"
develop_sha="$(git rev-parse "$DEVELOP_REF")"
info "main=$main_sha develop=$develop_sha"

ahead_count="$(git rev-list --count "${MAIN_REF}..${DEVELOP_REF}")"
if [[ "$ahead_count" != "0" ]]; then
  fail "$DEVELOP_REF has $ahead_count commit(s) not in $MAIN_REF (merge develop->main before release)"
fi

if ! git merge-base --is-ancestor "$DEVELOP_REF" "$MAIN_REF"; then
  fail "$DEVELOP_REF is not an ancestor of $MAIN_REF"
fi

info "PASS - release gate checks satisfied"
