#!/usr/bin/env python3
"""Provision the pinned sc-compose Python bindings used by publish-kit scripts."""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


# The one exact renderer version used by every Python invocation in this
# package. The published wheel provides bindings only; publisher agents use
# their consumer's CLI and do not import these bindings directly.
SC_COMPOSE_VERSION = "1.6.1"


def python_path(venv: Path) -> Path:
    """Return the platform-specific interpreter path in a virtual environment."""
    directory = "Scripts" if sys.platform == "win32" else "bin"
    executable = "python.exe" if sys.platform == "win32" else "python"
    return venv / directory / executable


def installed_version(python: Path) -> str | None:
    """Return installed distribution metadata before importing the binding module."""
    result = subprocess.run(
        [
            str(python),
            "-c",
            "from importlib.metadata import version; print(version('sc-compose'))",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def require_pinned_version(installed: str) -> None:
    """Fail unless the managed wheel exactly matches the package contract."""
    if installed != SC_COMPOSE_VERSION:
        raise SystemExit(
            "managed environment has incompatible sc-compose wheel: "
            f"found {installed!r}; required exactly {SC_COMPOSE_VERSION}."
        )


def install_pinned_wheel(python: Path) -> None:
    """Install the one wheel version the package supports."""
    subprocess.run(
        [
            str(python),
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            f"sc-compose=={SC_COMPOSE_VERSION}",
        ],
        check=True,
        stdout=sys.stderr,
    )


def provision_pinned_wheel(python: Path) -> None:
    """Install or replace a managed wheel until it exactly matches the pin."""
    existing = installed_version(python)
    if existing != SC_COMPOSE_VERSION:
        install_pinned_wheel(python)
        existing = installed_version(python)
    if existing is None:
        raise SystemExit(
            "managed environment has incompatible sc-compose wheel: "
            "installation completed but its version could not be determined"
        )
    require_pinned_version(existing)


def renderer_cli_path(venv: Path) -> Path:
    """Return the platform-specific renderer CLI path in a virtual environment."""
    directory = "Scripts" if sys.platform == "win32" else "bin"
    return venv / directory / "renderer"


def write_cli_wrapper(venv: Path, python: Path) -> Path:
    """Write a `sc-compose render` compatible CLI that uses the pinned wheel."""
    wrapper = renderer_cli_path(venv)
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    wrapper.write_text(
        f"""#!{python}
import argparse
import json
from pathlib import Path

import sc_compose


def main() -> int:
    parser = argparse.ArgumentParser(description="Pinned sc-compose renderer CLI")
    parser.add_argument("command", choices=["render"])
    parser.add_argument("--mode", required=True, choices=["file"])
    parser.add_argument("--root", required=True)
    parser.add_argument("--file", required=True)
    parser.add_argument("--var-file", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    variables = json.loads(Path(args.var_file).read_text(encoding="utf-8"))
    request = sc_compose.ComposeRequest(
        root=args.root,
        mode=sc_compose.ComposeMode.file(args.file),
        vars_input=variables,
        policy=sc_compose.ComposePolicy(strict_undeclared_variables=False),
    )
    Path(args.output).write_text(sc_compose.compose_file(request).rendered_text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
""",
        encoding="utf-8",
    )
    wrapper.chmod(0o755)
    return wrapper


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--venv", required=True, type=Path, help="managed virtual environment")
    parser.add_argument(
        "--write-cli",
        action="store_true",
        help="also write a sc-compose render compatible CLI into the venv",
    )
    args = parser.parse_args()
    venv = args.venv.resolve()
    python = python_path(venv)
    if not python.is_file():
        subprocess.run([sys.executable, "-m", "venv", str(venv)], check=True)

    provision_pinned_wheel(python)
    if args.write_cli:
        write_cli_wrapper(venv, python)
    print(python)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
