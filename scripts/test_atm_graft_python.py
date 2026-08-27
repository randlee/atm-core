#!/usr/bin/env python3
"""Build the extension with Maturin and prove that Python can import it."""

from __future__ import annotations

import pathlib
import os
import shutil
import subprocess
import sys
import tempfile
import venv

from graft_receiver_reclaim import verify_cross_process_receiver_reclaim


ROOT = pathlib.Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates" / "atm-graft-python"
HERMES_SOURCE = ROOT / "crates" / "hermes-atm" / "src" / "hermes_atm"


def verify_hermes_import_boundary() -> None:
    """Keep the Hermes package on the public PyO3 binding only."""
    for name in ("runtime.py", "native_tools.py"):
        source = (HERMES_SOURCE / name).read_text(encoding="utf-8")
        if "import atm_graft" not in source:
            raise AssertionError(f"{name} does not import the atm_graft binding")
        forbidden = ("graft_receiver_record", ".atm/graft", "write_receiver_record")
        if any(token in source for token in forbidden):
            raise AssertionError(f"{name} reaches into the retired graft file-record surface")


def main() -> None:
    verify_hermes_import_boundary()
    with tempfile.TemporaryDirectory(prefix="atm-graft-python-") as temp:
        venv_dir = pathlib.Path(temp) / "venv"
        wheel_dir = pathlib.Path(temp) / "wheel"
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        maturin = shutil.which("maturin")
        if maturin is None:
            raise RuntimeError("maturin is required for the ATM graft Python smoke test")
        wheel_dir.mkdir()
        subprocess.run(
            [
                maturin,
                "build",
                "--manifest-path",
                str(CRATE / "Cargo.toml"),
                "--release",
                "--out",
                str(wheel_dir),
            ],
            check=True,
            cwd=ROOT,
        )
        wheels = sorted(wheel_dir.glob("atm_graft*.whl"))
        if len(wheels) != 1:
            raise RuntimeError(f"expected one atm-graft wheel, found {wheels}")
        subprocess.run(
            [str(python), "-m", "pip", "install", str(wheels[0])],
            check=True,
            cwd=ROOT,
        )
        subprocess.run(
            [
                str(python),
                "-c",
                (
                    "import atm_graft; "
                    "address = atm_graft.PyAgentAddress('omega-prime', 'hermes', '1234'); "
                    "assert address.chat_id == '1234'; "
                    "assert str(address) == 'omega-prime:1234@hermes'; "
                    "nudge = atm_graft.PyNudge('01KX1TEST00000000000000000', address, 'body'); "
                    "assert str(nudge.source) == 'omega-prime:1234@hermes'; "
                    "assert hasattr(atm_graft, 'PyGraftSessionOptions'); "
                    "assert hasattr(atm_graft, 'PyGraftSessionSnapshot')"
                ),
            ],
            check=True,
            cwd=ROOT,
        )
    verify_cross_process_receiver_reclaim(ROOT)


if __name__ == "__main__":
    main()
