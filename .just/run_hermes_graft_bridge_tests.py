#!/usr/bin/env python3
"""Build the graft extension, then run the standard-library bridge tests."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import venv


ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates" / "atm-graft-python"
TESTS = CRATE / "tests"
PYTHON_SOURCE = CRATE / "python"


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="atm-graft-hermes-bridge-") as temp:
        venv_dir = Path(temp) / "venv"
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        maturin = shutil.which("maturin")
        if maturin is None:
            raise RuntimeError("maturin is required for the Hermes graft bridge test")
        env = {
            **os.environ,
            "VIRTUAL_ENV": str(venv_dir),
            "PATH": f"{python.parent}{os.pathsep}{os.environ['PATH']}",
            "PYTHONPATH": str(PYTHON_SOURCE),
        }
        subprocess.run(
            [maturin, "develop", "--manifest-path", str(CRATE / "Cargo.toml")],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [str(python), "-m", "unittest", "discover", "-s", str(TESTS), "-p", "test_hermes_bridge.py"],
            check=True,
            cwd=ROOT,
            env=env,
        )


if __name__ == "__main__":
    main()
