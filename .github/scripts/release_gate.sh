#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-final}"
RELEASE_REF="${2:-origin/main}"
RELEASE_CANDIDATE_TAG="${3:-}"
VERSION="${4:-${RELEASE_VERSION:-}}"
MANIFEST="${5:-release/publish-artifacts.toml}"
WORKSPACE_TOML="${6:-Cargo.toml}"

fail() {
  echo "release-gate: FAIL - $*" >&2
  exit 1
}

info() {
  echo "release-gate: $*"
}

case "$MODE" in
  readiness|final) ;;
  *) fail "mode must be readiness or final (got: $MODE)" ;;
esac

[[ -n "$RELEASE_CANDIDATE_TAG" ]] || fail "release-candidate tag is required (arg 3)"
[[ "$RELEASE_CANDIDATE_TAG" =~ ^release-candidate-v[0-9]+\.[0-9]+\.[0-9]+$ ]] \
  || fail "invalid release-candidate tag: $RELEASE_CANDIDATE_TAG"
[[ -n "$VERSION" ]] || fail "release version is required (arg 4 or RELEASE_VERSION)"
[[ "$RELEASE_CANDIDATE_TAG" == "release-candidate-v${VERSION}" ]] \
  || fail "release-candidate tag $RELEASE_CANDIDATE_TAG does not match version $VERSION"

info "fetching refs and tags"
git fetch origin --prune --tags >/dev/null 2>&1 || fail "git fetch failed"

git rev-parse --verify "$RELEASE_REF" >/dev/null 2>&1 || fail "missing release ref: $RELEASE_REF"
git rev-parse --verify "refs/tags/$RELEASE_CANDIDATE_TAG" >/dev/null 2>&1 \
  || fail "missing release-candidate tag: $RELEASE_CANDIDATE_TAG"

release_sha="$(git rev-parse "$RELEASE_REF")"
candidate_sha="$(git rev-parse "${RELEASE_CANDIDATE_TAG}^{commit}")"
info "mode=$MODE release_ref=$RELEASE_REF release=$release_sha candidate_tag=$RELEASE_CANDIDATE_TAG candidate=$candidate_sha version=$VERSION"

if ! git merge-base --is-ancestor "$RELEASE_CANDIDATE_TAG" "$RELEASE_REF"; then
  fail "$RELEASE_CANDIDATE_TAG ($candidate_sha) is not an ancestor of $RELEASE_REF ($release_sha)"
fi

if [[ "$MODE" == "final" && "$RELEASE_REF" != "origin/main" ]]; then
  fail "final mode must validate origin/main (got: $RELEASE_REF)"
fi

python3 .github/scripts/release_artifacts.py check-version-unpublished \
  --manifest "$MANIFEST" \
  --version "$VERSION" >/dev/null

python3 .github/scripts/release_artifacts.py verify-version-lockstep \
  --manifest "$MANIFEST" \
  --workspace-toml "$WORKSPACE_TOML" >/dev/null

info "PASS - release gate checks satisfied"
