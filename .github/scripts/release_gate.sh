#!/usr/bin/env bash
set -euo pipefail

MAIN_REF="${1:-origin/main}"
DEVELOP_REF="${2:-origin/develop}"
VERSION="${3:-${RELEASE_VERSION:-}}"
MANIFEST="${4:-release/publish-artifacts.toml}"

fail() {
  echo "release-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "release-gate: $*"
}

[[ -n "$VERSION" ]] || fail "release version is required (arg 3 or RELEASE_VERSION)"

info "fetching refs and tags"
git fetch origin --prune --tags >/dev/null 2>&1 || fail "git fetch failed"
# actions/checkout only fetches the checked-out branch; explicitly fetch develop
git fetch origin "${DEVELOP_REF#*/}" >/dev/null 2>&1 || fail "git fetch ${DEVELOP_REF#*/} failed"

git rev-parse --verify "$MAIN_REF" >/dev/null 2>&1 || fail "missing ref: $MAIN_REF"
git rev-parse --verify "$DEVELOP_REF" >/dev/null 2>&1 || fail "missing ref: $DEVELOP_REF"

main_sha="$(git rev-parse "$MAIN_REF")"
develop_sha="$(git rev-parse "$DEVELOP_REF")"
info "main=$main_sha develop=$develop_sha version=$VERSION"

if ! git diff --quiet "$MAIN_REF" "$DEVELOP_REF"; then
  fail "$DEVELOP_REF differs in content from $MAIN_REF (merge develop->main before release)"
fi

python3 .github/scripts/release_artifacts.py check-version-unpublished \
  --manifest "$MANIFEST" \
  --version "$VERSION" >/dev/null

python3 .github/scripts/release_artifacts.py verify-version-lockstep \
  --manifest "$MANIFEST" \
  --workspace-toml Cargo.toml >/dev/null

info "PASS - release gate checks satisfied"
