"""Bound subprocesses used by the package's installer and installed-suite tests."""

import subprocess

import pytest

COMMAND_TIMEOUT = 30
HANG_GUARD_SECONDS = 60
SUITE_TIMEOUT = 1200  # generous hang backstop, shorter than the 30-minute CI job timeout
OUTPUT_TAIL = 16000


def run_command(command, *, timeout: float = COMMAND_TIMEOUT, check=False, runner=None, **kwargs):
    if not kwargs.get("capture_output"):
        kwargs.setdefault("stdout", subprocess.PIPE)
        kwargs.setdefault("stderr", subprocess.PIPE)
    if kwargs.get("input") is None and "stdin" not in kwargs:
        kwargs["stdin"] = subprocess.DEVNULL
    try:
        return (runner or subprocess.run)(command, timeout=timeout, check=check, **kwargs)
    except subprocess.TimeoutExpired as error:
        def tail(output):
            if isinstance(output, bytes):
                output = output.decode("utf-8", errors="replace")
            return (output or "")[-OUTPUT_TAIL:]

        pytest.fail(
            f"Command {command!r} timed out after {timeout}s\n"
            f"stdout tail:\n{tail(error.stdout)}\nstderr tail:\n{tail(error.stderr)}",
            pytrace=False,
        )
