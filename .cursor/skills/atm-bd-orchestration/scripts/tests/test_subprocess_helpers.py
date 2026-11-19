"""The shared subprocess guard reports actionable timeout context."""

import subprocess

import pytest

from subprocess_helpers import run_command


def test_timeout_diagnostic_names_command_cwd_and_timeout(tmp_path):
    command = ["controlled-child", "--wait"]

    def timed_out(actual_command, *, timeout, **kwargs):
        raise subprocess.TimeoutExpired(
            actual_command,
            timeout=timeout,
            output=b"partial stdout",
            stderr=b"partial stderr",
        )

    with pytest.raises(pytest.fail.Exception) as error:
        run_command(
            command,
            timeout=0.25,
            cwd=tmp_path,
            capture_output=True,
            text=True,
            runner=timed_out,
        )

    message = str(error.value)
    assert "SUBPROCESS.TIMEOUT" in message
    assert f"command={command!r}" in message
    assert f"cwd={str(tmp_path)!r}" in message
    assert "timeout=0.25s" in message
    assert "partial stdout" in message
    assert "partial stderr" in message
