#!/usr/bin/env bash
set -euo pipefail

python3 - <<'PY'
import tomllib
from pathlib import Path

data = tomllib.loads(Path("release/publish-artifacts.toml").read_text(encoding="utf-8"))
orders = [item["publish_order"] for item in data["crates"]]
if len(orders) != len(set(orders)):
    raise SystemExit("duplicate publish_order values detected")
if orders != sorted(orders):
    raise SystemExit("publish_order values must already be sorted in the manifest")
print("publish order validation passed")
PY
