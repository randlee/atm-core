#!/usr/bin/env python3
"""Build both installed wheels, then run the Hermes ATM contract tests."""

from __future__ import annotations

import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import venv
import zipfile


ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates" / "atm-graft-python"
TESTS = ROOT / "crates" / "hermes-atm" / "tests"
HERMES_PACKAGE = ROOT / "crates" / "hermes-atm"
WHEEL_OUTPUT_DIR_ENV = "ATM_WHEEL_OUTPUT_DIR"


def project_dependency_requirement(manifest_path: Path, package_name: str) -> str:
    """Return one declared project dependency, preserving its version constraint."""

    metadata = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    dependencies = metadata.get("project", {}).get("dependencies", [])
    for dependency in dependencies:
        if isinstance(dependency, str) and dependency.split("=", 1)[0].split("<", 1)[0].split(">", 1)[0] == package_name:
            return dependency
    raise RuntimeError(f"{manifest_path} does not declare required dependency {package_name}")


def require_universal_python_wheel(wheel_path: Path) -> None:
    if not wheel_path.name.endswith("-py3-none-any.whl"):
        raise RuntimeError(
            "expected hermes-atm to ship one universal Python wheel, found "
            f"{wheel_path.name}"
        )


def bridge_test_environment(venv_dir: Path, python: Path) -> dict[str, str]:
    """Build an isolated environment with no checkout source-path injection."""

    environment = {
        **os.environ,
        "VIRTUAL_ENV": str(venv_dir),
        "PATH": f"{python.parent}{os.pathsep}{os.environ['PATH']}",
    }
    environment.pop("PYTHONPATH", None)
    return environment


def wheel_output_dir(temp_dir: Path) -> Path:
    """Return a retained CI output directory, or a temporary local one."""

    configured = os.environ.get(WHEEL_OUTPUT_DIR_ENV)
    output_dir = Path(configured).resolve() if configured else temp_dir / "wheels"
    output_dir.mkdir(parents=True, exist_ok=True)
    return output_dir


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="atm-graft-hermes-bridge-") as temp:
        venv_dir = Path(temp) / "venv"
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        maturin = shutil.which("maturin")
        if maturin is None:
            raise RuntimeError("maturin is required for the Hermes graft bridge test")
        env = bridge_test_environment(venv_dir, python)
        wheel_dir = wheel_output_dir(Path(temp))
        subprocess.run(
            [str(python), "-m", "pip", "install", "--quiet", "wheel"],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [maturin, "build", "--manifest-path", str(CRATE / "Cargo.toml"), "--out", str(wheel_dir)],
            check=True,
            cwd=ROOT,
            env=env,
        )
        graft_wheels = sorted(wheel_dir.glob("atm_graft*.whl"))
        if len(graft_wheels) != 1:
            raise RuntimeError(f"expected one atm-graft wheel, found {len(graft_wheels)}")
        with zipfile.ZipFile(graft_wheels[0]) as wheel:
            wheel_files = set(wheel.namelist())
        retired_sources = {
            "atm_graft_hermes_adapter/__init__.py",
            "atm_graft_hermes_bridge/__init__.py",
            "atm_graft_hermes_loader/__init__.py",
        }
        shipped_retired_sources = retired_sources & wheel_files
        if shipped_retired_sources:
            raise RuntimeError(f"generic wheel shipped Hermes sources: {sorted(shipped_retired_sources)}")
        subprocess.run(
            [
                str(python),
                "-m",
                "pip",
                "wheel",
                "--no-deps",
                # Keep build isolation enabled: CPython 3.14 venvs do not
                # include setuptools, so pip must bootstrap the declared
                # setuptools.build_meta backend in its temporary build env.
                "-w",
                str(wheel_dir),
                str(HERMES_PACKAGE),
            ],
            check=True,
            cwd=ROOT,
            env=env,
        )
        hermes_wheels = sorted(wheel_dir.glob("hermes_atm*.whl"))
        if len(hermes_wheels) != 1:
            raise RuntimeError(f"expected one hermes-atm wheel, found {len(hermes_wheels)}")
        require_universal_python_wheel(hermes_wheels[0])
        # The generic graft wheel owns the strict Pydantic ingress models.
        # Install that declared runtime dependency explicitly because this
        # contract runner intentionally installs both project wheels with
        # ``--no-deps`` to avoid resolving an unrelated published ATM wheel.
        subprocess.run(
            [
                str(python),
                "-m",
                "pip",
                "install",
                project_dependency_requirement(CRATE / "pyproject.toml", "pydantic"),
            ],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [str(python), "-m", "pip", "install", "--no-deps", str(graft_wheels[0]), str(hermes_wheels[0])],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [
                str(python),
                "-c",
                "import atm_graft, hermes_atm",
            ],
            check=True,
            cwd=ROOT,
            env=env,
        )
        subprocess.run(
            [str(python), "-m", "unittest", "discover", "-s", str(TESTS), "-p", "test_*.py"],
            check=True,
            cwd=ROOT,
            env=env,
        )


if __name__ == "__main__":
    main()
