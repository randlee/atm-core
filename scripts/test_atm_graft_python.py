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


ROOT = pathlib.Path(__file__).resolve().parents[1]
CRATE = ROOT / "crates" / "atm-graft-python"


def verify_cross_process_receiver_reclaim(root: pathlib.Path) -> None:
    """Prove that a crashed receiver owner releases its OS lock for a new process."""
    with tempfile.TemporaryDirectory(prefix="atm-graft-owner-") as fixture_root:
        environment = {**os.environ, "ATM_GRAFT_RECLAIM_CHILD_ROOT": fixture_root}
        for test_name in (
            "child_owner_exits_without_drop",
            "parent_reclaims_child_owner_lock",
        ):
            subprocess.run(
                [
                    "cargo",
                    "test",
                    "-p",
                    "agent-team-mail-core",
                    "--test",
                    "graft_receiver_ownership",
                    test_name,
                    "--",
                    "--ignored",
                    "--exact",
                ],
                check=True,
                cwd=root,
                env=environment,
            )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="atm-graft-python-") as temp:
        venv_dir = pathlib.Path(temp) / "venv"
        venv.EnvBuilder(with_pip=True).create(venv_dir)
        python = venv_dir / ("Scripts/python.exe" if sys.platform == "win32" else "bin/python")
        maturin = shutil.which("maturin")
        if maturin is None:
            raise RuntimeError("maturin is required for the ATM graft Python smoke test")
        subprocess.run(
            [maturin, "develop", "--manifest-path", str(CRATE / "Cargo.toml")],
            check=True,
            cwd=ROOT,
            env={
                **os.environ,
                "VIRTUAL_ENV": str(venv_dir),
                "PATH": f"{python.parent}{os.pathsep}{os.environ['PATH']}",
            },
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
