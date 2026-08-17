#!/usr/bin/env bash
set -euo pipefail

MAIN_REF="${1:-origin/main}"
DEVELOP_REF="${2:-origin/develop}"
VERSION="${3:-${RELEASE_VERSION:-}}"
MANIFEST="${4:-release/publish-artifacts.toml}"
RELEASE_TAG="${5:-v${VERSION}}"
RELEASE_TARGET="${6:-production}"

fail() {
  echo "release-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "release-gate: $*"
}

[[ -n "$VERSION" ]] || fail "release version is required (arg 3 or RELEASE_VERSION)"
[[ "$RELEASE_TAG" == "v${VERSION}" ]] || fail "release tag ${RELEASE_TAG} does not match version ${VERSION}"
[[ "$RELEASE_TARGET" == "testpypi" || "$RELEASE_TARGET" == "production" ]] || fail \
  "release target must be testpypi or production (got: ${RELEASE_TARGET})"

info "fetching refs and tags"
git fetch origin --prune --tags >/dev/null 2>&1 || fail "git fetch failed"
# actions/checkout fetches only the selected ref; fetch develop explicitly so
# the convergence test always compares remote state rather than runner cache.
git fetch origin "${DEVELOP_REF#*/}" >/dev/null 2>&1 || fail "git fetch ${DEVELOP_REF#*/} failed"

git rev-parse --verify "$MAIN_REF" >/dev/null 2>&1 || fail "missing ref: $MAIN_REF"
git rev-parse --verify "$DEVELOP_REF" >/dev/null 2>&1 || fail "missing ref: $DEVELOP_REF"

main_sha="$(git rev-parse "$MAIN_REF")"
develop_sha="$(git rev-parse "$DEVELOP_REF")"
info "main=$main_sha develop=$develop_sha version=$VERSION"

if ! git merge-base --is-ancestor "$DEVELOP_REF" "$MAIN_REF"; then
  fail "$DEVELOP_REF has commits not in $MAIN_REF (merge develop->main before release)"
fi

# Tags are immutable release receipts. Never retag: an existing remote tag
# must already name the exact main commit that this workflow will release.
if [[ "$RELEASE_TARGET" == "production" ]]; then
  remote_tag_sha="$(git ls-remote --tags origin "refs/tags/${RELEASE_TAG}^{}" | awk 'NR == 1 { print $1 }')"
  if [[ -z "$remote_tag_sha" ]]; then
    remote_tag_sha="$(git ls-remote --tags origin "refs/tags/${RELEASE_TAG}" | awk 'NR == 1 { print $1 }')"
  fi
  if [[ -n "$remote_tag_sha" && "$remote_tag_sha" != "$main_sha" ]]; then
    fail "tag ${RELEASE_TAG} exists but points to ${remote_tag_sha}, not ${MAIN_REF} (${main_sha}); never retag"
  fi
  info "tag ${RELEASE_TAG} is absent or already points to ${MAIN_REF}"
fi

python3 scripts/release_artifacts.py check-version-unpublished \
  --manifest "$MANIFEST" \
  --version "$VERSION" >/dev/null

python3 scripts/release_artifacts.py verify-readme-version \
  --manifest "$MANIFEST" \
  --workspace-toml Cargo.toml \
  --readme README.md >/dev/null

info "PASS - release gate checks satisfied"
