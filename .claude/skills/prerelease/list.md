# List prereleases

List published, non-draft GitHub prereleases whose tags match the
manifest-declared prefix. This mode is read-only.

```bash
set -euo pipefail
manifest="release/publish-artifacts.toml"
prefix="$(python3 -c 'import sys,tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["prerelease"]["tag_prefix"])' "$manifest")"
releases="$(gh release list --limit 100 --json tagName,name,isDraft,isPrerelease,publishedAt)"
PRERELEASE_TAG_PREFIX="$prefix" PRERELEASE_RELEASES_JSON="$releases" python3 - <<'PY'
import json
import os

prefix = os.environ["PRERELEASE_TAG_PREFIX"]
releases = json.loads(os.environ["PRERELEASE_RELEASES_JSON"])
selected = [
    release
    for release in releases
    if release.get("isPrerelease")
    and not release.get("isDraft")
    and release.get("tagName", "").startswith(prefix)
]
print(json.dumps(selected, indent=2))
PY
```

An empty JSON array means the repository has no matching published
prereleases.
