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
import zipfile


ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates" / "atm-graft-python"
TESTS = CRATE / "tests"


def bridge_test_environment(venv_dir: Path, python: Path) -> dict[str, str]:
    """Build an isolated environment with no checkout source-path injection."""

    environment = {
        **os.environ,
        "VIRTUAL_ENV": str(venv_dir),
        "PATH": f"{python.parent}{os.pathsep}{os.environ['PATH']}",
    }
    environment.pop("PYTHONPATH", None)
    return environment


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="atm-graft-hermes-bridge-") as temp:
        venv_dir = Path(temp) / "venv"
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        maturin = shutil.which("maturin")
        if maturin is None:
            raise RuntimeError("maturin is required for the Hermes graft bridge test")
        env = bridge_test_environment(venv_dir, python)
        wheel_dir = Path(temp) / "wheels"
        wheel_dir.mkdir()
        subprocess.run(
            [maturin, "build", "--manifest-path", str(CRATE / "Cargo.toml"), "--out", str(wheel_dir)],
            check=True,
            cwd=ROOT,
            env=env,
        )
        wheels = sorted(wheel_dir.glob("*.whl"))
        if len(wheels) != 1:
            raise RuntimeError(f"expected one atm-graft wheel, found {len(wheels)}")
        with zipfile.ZipFile(wheels[0]) as wheel:
            wheel_files = set(wheel.namelist())
        expected_sources = {
            "atm_graft_hermes_adapter/__init__.py",
            "atm_graft_hermes_bridge/__init__.py",
            "atm_graft_hermes_loader/__init__.py",
        }
        missing_sources = expected_sources - wheel_files
        if missing_sources:
            raise RuntimeError(f"wheel omitted Hermes sources: {sorted(missing_sources)}")
        subprocess.run(
            [str(python), "-m", "pip", "install", "--no-deps", str(wheels[0])],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [
                str(python),
                "-c",
                "import atm_graft, atm_graft_hermes_adapter, atm_graft_hermes_bridge, atm_graft_hermes_loader",
            ],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [str(python), str(ROOT / "scripts" / "phase-ai" / "run-hermes-steer-smoke.py"), "--fixture"],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [str(python), "-m", "unittest", "discover", "-s", str(TESTS), "-p", "test_hermes_*.py"],
            check=True,
            cwd=ROOT,
            env=env,
        )


if __name__ == "__main__":
    main()
