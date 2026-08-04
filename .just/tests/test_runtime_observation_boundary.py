from pathlib import Path
import subprocess
import sys


def test_guard_accepts_phase_aj_sources() -> None:
    root = Path(__file__).resolve().parents[2]
    result = subprocess.run(
        [sys.executable, str(root / ".just/check_runtime_observation_boundary.py")],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
