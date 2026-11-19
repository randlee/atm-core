"""Bound subprocesses and isolate temporary Git fixtures used by package tests."""

import os
import subprocess
from pathlib import Path

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
        cwd = kwargs.get("cwd")
        cwd_description = repr(str(cwd)) if cwd is not None else "<inherited>"

        def tail(output):
            if isinstance(output, bytes):
                output = output.decode("utf-8", errors="replace")
            return (output or "")[-OUTPUT_TAIL:]

        pytest.fail(
            f"SUBPROCESS.TIMEOUT: command={command!r}, cwd={cwd_description}, timeout={timeout}s\n"
            f"stdout tail:\n{tail(error.stdout)}\nstderr tail:\n{tail(error.stderr)}",
            pytrace=False,
        )


def isolated_git_env(root):
    """Return a host-independent Git environment rooted outside the fixture repository."""
    root = Path(root)
    home = root / ".git-fixture-home"
    global_config = root / ".git-fixture-global-config"
    system_config = root / ".git-fixture-system-config"
    template = root / ".git-fixture-template"
    hooks = root / ".git-fixture-hooks"
    home.mkdir(parents=True, exist_ok=True)
    template.mkdir(parents=True, exist_ok=True)
    hooks.mkdir(parents=True, exist_ok=True)
    global_config.touch(exist_ok=True)
    system_config.touch(exist_ok=True)

    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update({
        "HOME": str(home),
        "XDG_CONFIG_HOME": str(home / "xdg"),
        "GIT_CONFIG_GLOBAL": str(global_config),
        "GIT_CONFIG_SYSTEM": str(system_config),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TEMPLATE_DIR": str(template),
        "ATM_TEST_GIT_HOOKS_PATH": str(hooks),
    })
    return env, template


def run_fixture_git(command, *, env, **kwargs):
    """Run a bounded Git fixture command with hooks disabled inside the fixture root."""
    args = list(command)
    if args and args[0] == "git":
        args[1:1] = ["-c", f"core.hooksPath={env['ATM_TEST_GIT_HOOKS_PATH']}"]
    kwargs.setdefault("timeout", HANG_GUARD_SECONDS)
    kwargs.setdefault("check", True)
    kwargs.setdefault("capture_output", True)
    kwargs.setdefault("text", True)
    return run_command(args, env=env, **kwargs)


def set_fixture_hooks_path(repository, *, env):
    """Persist the empty fixture hook directory for Git commands run by code under test."""
    run_fixture_git(
        ["git", "-C", str(repository), "config", "--local", "core.hooksPath", env["ATM_TEST_GIT_HOOKS_PATH"]],
        env=env,
    )
