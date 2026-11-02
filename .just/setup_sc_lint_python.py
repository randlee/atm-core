#!/usr/bin/env python3
from __future__ import annotations

import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    with (ROOT / "tools" / "bootstrap.toml").open("rb") as source:
        bootstrap = tomllib.load(source)
    minimum_sc_compose = bootstrap["sc-compose"]["version"]
    if sys.platform == "win32":
        python = ROOT / ".sc-lint" / "venv" / "Scripts" / "python.exe"
    else:
        python = ROOT / ".sc-lint" / "venv" / "bin" / "python"
    if not python.is_file():
        raise SystemExit(f"sc-lint Python environment is missing: {python}; run `just setup` first")

    subprocess.run(
        [
            str(python), "-m", "pip", "install", "--disable-pip-version-check", "--no-deps",
            "--requirement", str(ROOT / "tools" / "bootstrap-requirements.txt"),
        ],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(
        [
            str(python), "-m", "pip", "install", "--disable-pip-version-check", "--no-deps",
            f"sc-compose>={minimum_sc_compose},<2.0.0",
        ],
        cwd=ROOT,
        check=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
