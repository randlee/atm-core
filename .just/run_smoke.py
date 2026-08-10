"""Dispatch the canonical smoke runners without relying on a host shell."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SPECIAL_RUNNERS = {
    "peer-pair": ROOT / "scripts" / "smoke" / "run_peer_pair.py",
    "inbound-peer": ROOT / "scripts" / "smoke" / "run_inbound_peer_smoke.py",
    "inbound-peer-combine": ROOT / "scripts" / "smoke" / "combine_inbound_peer_smoke.py",
    "graft-hermes": ROOT / "scripts" / "phase-ai" / "run_hermes_graft_live.py",
}


def main() -> int:
    if len(sys.argv) < 2:
        print("smoke feature is required", file=sys.stderr)
        return 2

    feature, *args = sys.argv[1:]
    runner = SPECIAL_RUNNERS.get(feature)
    command = [sys.executable, str(runner), *args] if runner else [
        sys.executable,
        str(ROOT / "scripts" / "smoke" / "run_feature_smoke.py"),
        feature,
        *args,
    ]
    return subprocess.run(command, cwd=ROOT, check=False).returncode


if __name__ == "__main__":
    raise SystemExit(main())
